use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use futures::future::{try_join_all, Future};
use log::{debug, error, info};
use uuid::Uuid;

use crate::crustasyncfs::base::{ContentHash, FileSystem, Node};
use crate::error::Result;

#[derive(Clone, Debug)]
pub enum Task {
    Move { from: PathBuf, to: PathBuf },
    Upload { path: PathBuf },
    CreateDir { path: PathBuf },
    Delete { path: PathBuf },
}

impl Node {
    // Prefix file with ffff, dir with dddd
    // This is to distinguish between empty files and empty dirs
    pub fn node_hash(&self) -> ContentHash {
        let mut hash = self.content_hash;
        if self.is_file() {
            hash[0..4].fill(b'f');
        } else {
            hash[0..4].fill(b'd');
        }
        hash
    }
}

// Build a map from node hash to a vector of nodes with that content
fn build_node_hash_table(tree: &Node) -> HashMap<ContentHash, Vec<&Node>> {
    let mut table: HashMap<ContentHash, Vec<&Node>> = HashMap::new();
    for node in tree {
        let node_hash = node.node_hash();
        if let Some(nodes) = table.get_mut(&node_hash) {
            nodes.push(node);
        } else {
            table.insert(node_hash, vec![node]);
        }
    }
    table
}

fn build_path_hash_table(tree: &Node) -> HashMap<PathBuf, &Node> {
    let mut table = HashMap::new();
    for node in tree {
        table.insert(node.path.clone(), node);
    }
    table
}

// Return tasks to turn dst_tree into src_tree
// The tasks are divided into priority classes
// All tasks of the same priority must be completed before processing lower priority tasks
pub fn build_task_queue(src_tree: &Node, dst_tree: &Node) -> Vec<Vec<Task>> {
    debug!("Start building tasks");

    let empty_path = Path::new("");

    // Create new dir must happen before upload & move
    // Move must happen before delete dir
    let mut queue_0 = vec![]; // move to tmp files
    let mut queue_1 = vec![]; // delete dirs whose going to be changed to files
    let mut queue_2 = vec![]; // create dir
                              // queue_3: move
    let mut queue_4 = vec![]; // upload
                              // queue_5: delete

    let mut src_content_table = build_node_hash_table(src_tree);
    // let dst_path_table = build_path_hash_table(&dst_tree);
    let src_path_table = build_path_hash_table(src_tree);

    let mut to_move = HashMap::new();
    let mut to_del = HashMap::new();

    // Move file & directories + delete files
    debug!("Finding files & dirs to move and delete");
    for dst_node in dst_tree {
        // TODO handle circular rename
        // src and dst share the same file/directory
        if let Some(src_nodes) = src_content_table.get_mut(&dst_node.node_hash()) {
            if !src_nodes.is_empty() {
                // if any of the src_nodes has the same path -> don't do anything
                if let Some(idx) = src_nodes.iter().position(|n| n.path == dst_node.path) {
                    src_nodes.remove(idx);
                    continue;
                }
                // otherwise, move 1 node in the list
                to_move.insert(
                    dst_node.path.clone(),
                    (
                        Task::Move {
                            from: dst_node.path.clone(),
                            to: src_nodes.pop().unwrap().path.clone(),
                        },
                        dst_node.is_file(),
                    ),
                );
                continue;
            }
        };

        // content not found in src tree
        // is file:
        //      if path in dst fs is a dir: del with highest priority
        //      else delete later
        // is dir & is not root dir -> delete
        if dst_node.is_file() {
            to_del.insert(
                dst_node.path.clone(),
                (
                    Task::Delete {
                        path: dst_node.path.clone(),
                    },
                    true,
                ),
            );
        } else if dst_node.path != empty_path {
            // queue_3.push(Task::Delete {
            //     path: dst_node.path.clone(),
            // })
            to_del.insert(
                dst_node.path.clone(),
                (
                    Task::Delete {
                        path: dst_node.path.clone(),
                    },
                    false,
                ),
            );
        }
    }

    // Create dir & Upload new files
    debug!("Finding new dir to create & new file to write");
    for new_nodes in src_content_table.values() {
        for new in new_nodes {
            if new.is_file() {
                if let Some((del_task, is_dst_node_file)) = to_del.get(&new.path) {
                    if *is_dst_node_file {
                        // dst path is file
                        // newly uploaded file will override, no need to delete
                        to_del.remove(&new.path);
                    } else if let Task::Delete { path } = del_task {
                        // check if the dir to-be-overwritten have any to-be-moved descendant
                        // move the descendants to tmp files, then edit their move tasks
                        //
                        to_move.iter_mut().for_each(|(k, (task, _))| {
                            if !k.starts_with(path) {
                                return;
                            }
                            let uuid = Uuid::new_v4().to_string();
                            let temp_file_name = String::from(".crustasync-").add(&uuid);
                            let temp_path_buf = PathBuf::from(temp_file_name);
                            if let Task::Move { from, to: _ } = task {
                                queue_0.push(Task::Move {
                                    from: from.clone(),
                                    to: temp_path_buf.clone(),
                                });
                                from.clear();
                                from.push(temp_path_buf);
                            }
                        });
                        queue_1.push(Task::Delete { path: path.clone() });
                        to_del.remove(&new.path);
                    }
                }
                queue_4.push(Task::Upload {
                    path: new.path.clone(),
                });
            } else {
                if let Some((_del_task, is_dst_node_file)) = to_del.get(&new.path) {
                    // if the path is already a dir, no need to del current one then create new one
                    if !is_dst_node_file {
                        to_del.remove(&new.path);
                        continue;
                    }
                    // if the path is file, need to delete it with higher priority
                    to_del.remove(&new.path);
                    queue_1.push(Task::Delete {
                        path: new.path.clone(),
                    })
                }
                if new.path != empty_path {
                    queue_2.push(Task::CreateDir {
                        path: new.path.clone(),
                    });
                }
            }
        }
    }

    debug!("Sort and dedup tasks");

    // dedup
    queue_0 = dedup_move_tasks(queue_0);
    queue_1 = dedup_del_tasks(queue_1);

    // make sure that parent directories are created first
    queue_2.sort_by_key(|task| {
        if let Task::CreateDir { path } = task {
            String::from(path.to_str().unwrap())
        } else {
            String::new()
        }
    });

    // Put the remaining move task to queue 3
    let mut queue_3 = to_move.into_iter().map(|(_, (task, _))| task).collect();
    queue_3 = dedup_move_tasks(queue_3);

    // Put the remaining delete task to queue 5
    // Make sure that we don't delete any new files / dirs
    // and that we don't delete nested files / dirs that are already deleted in queue1
    let mut queue_5 = to_del
        .into_iter()
        .filter_map(|(_, (task, _))| {
            if let Task::Delete { path } = &task {
                let deleted_recursively_in_q1 = queue_1.iter().any(|task| {
                    if let Task::Delete {
                        path: q1_deleted_path,
                    } = task
                    {
                        return path.starts_with(q1_deleted_path);
                    }
                    false // never happens
                });
                if !src_path_table.contains_key(path) && !deleted_recursively_in_q1 {
                    return Some(task);
                }
            }
            None
        })
        .collect();
    queue_5 = dedup_del_tasks(queue_5);

    // Make sure to delete all nested node before deleting the parent dir
    queue_5.sort_by_key(|task| {
        let s = if let Task::Delete { path } = task {
            String::from(path.to_str().unwrap())
        } else {
            String::new()
        };
        Reverse(s)
    });

    let result = vec![queue_0, queue_1, queue_2, queue_3, queue_4, queue_5];

    let total = result
        .iter()
        .map(|q| q.len())
        .reduce(|a, b| a + b)
        .unwrap_or(0);
    debug!("Build tasks done. Total {} task(s)", total);

    result
}

fn dedup_move_tasks(tasks: Vec<Task>) -> Vec<Task> {
    let mut from_paths = vec![];
    let mut to_paths = vec![];
    for task in &tasks {
        if let Task::Move { from, to } = task {
            from_paths.push(from.clone());
            to_paths.push(to.clone());
        }
    }

    tasks
        .into_iter()
        .filter(|task: &Task| {
            if let Task::Move { from, to } = task {
                for pb in &from_paths {
                    if from.starts_with(pb) && from.ne(pb) {
                        return false;
                    }
                }
                for pb in &to_paths {
                    if to.starts_with(pb) && to.ne(pb) {
                        return false;
                    }
                }
            }
            true
        })
        .collect()
}

fn dedup_del_tasks(tasks: Vec<Task>) -> Vec<Task> {
    let paths: Vec<PathBuf> = tasks
        .iter()
        .filter_map(|task: &Task| {
            if let Task::Delete { path } = task {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect();

    tasks
        .into_iter()
        .filter(|task: &Task| {
            if let Task::Delete { path } = task {
                for pb in &paths {
                    if path.starts_with(pb) && path.ne(pb) {
                        return false;
                    }
                }
            }
            true
        })
        .collect()
}

async fn process_move(fs: Arc<dyn FileSystem>, from: &Path, to: &Path) -> Result<()> {
    info!("Start moving from {:?} to {:?}", from, to);
    let res = fs.mv(from, to).await;
    if res.is_err() {
        error!("Error moving from {:?} to {:?}", from, to);
    } else {
        info!("Done moving from {:?} to {:?}", from, to);
    }
    res
}

async fn process_upload(
    src_fs: Arc<dyn FileSystem>,
    dst_fs: Arc<dyn FileSystem>,
    path: &Path,
) -> Result<()> {
    info!("Start uploading to {:?}", path);
    let content = src_fs.read(path).await?;
    let res = dst_fs.write(path, &content).await;

    if res.is_err() {
        error!("Error uploading to {:?}", path);
    } else {
        info!("Done uploading to {:?}", path);
    }
    res
}

async fn process_create_dir(fs: Arc<dyn FileSystem>, path: &Path) -> Result<()> {
    info!("Start creating dir to {:?}", path);
    let res = fs.mkdir(path).await;
    if res.is_err() {
        error!("Error creating dir {:?}", path);
    } else {
        info!("Done creating dir {:?}", path);
    }
    res
}

async fn process_delete(fs: Arc<dyn FileSystem>, path: &Path) -> Result<()> {
    info!("Start deleting {:?}", path);
    let res = fs.rm(path).await;
    if res.is_err() {
        error!("Error deleting {:?}", path);
    } else {
        info!("Done deleting {:?}", path);
    }
    res
}

pub async fn process_tasks(
    src_fs: Arc<dyn FileSystem>,
    dst_fs: Arc<dyn FileSystem>,
    queues: &Vec<Vec<Task>>,
) -> Result<()> {
    info!("Start processing tasks");
    for queue in queues {
        let futures = queue.iter().map(|task: &Task| {
            let dst_fs = dst_fs.clone();
            let box_future: Pin<Box<dyn Future<Output = Result<()>>>> = match task {
                Task::Move { from, to } => Box::pin(process_move(dst_fs, from, to)),
                Task::Upload { path } => Box::pin(process_upload(src_fs.clone(), dst_fs, path)),
                Task::CreateDir { path } => Box::pin(process_create_dir(dst_fs, path)),
                Task::Delete { path } => Box::pin(process_delete(dst_fs, path)),
            };
            box_future
        });
        try_join_all(futures).await?;
    }
    info!("Processing tasks done");
    Ok(())
}
