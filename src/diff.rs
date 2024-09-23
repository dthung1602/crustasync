use futures::future::{try_join_all, Future};
use log::{error, info};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display};
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tokio::io;
use uuid::Uuid;

use crate::crustasyncfs::base::{ContentHash, FileSystem, Node};

#[derive(Clone, Debug)]
pub enum Task {
    Move { from: PathBuf, to: PathBuf },
    Upload { path: PathBuf },
    CreateDir { path: PathBuf },
    Delete { path: PathBuf },
}

fn build_content_hash_table(tree: &Node) -> HashMap<ContentHash, &Node> {
    // TODO files with same content
    let mut table = HashMap::new();
    for node in tree {
        table.insert(node.content_hash, node);
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
    let empty_path = Path::new("");

    // ---> TODO exclude .crustasync file?
    // Create new dir must happen before upload & move
    // Move must happen before delete dir
    let mut queue_0 = vec![]; // move to tmp files
    let mut queue_1 = vec![]; // delete dirs whose going to be changed to files
    let mut queue_2 = vec![]; // create dir
    let mut queue_3 = vec![]; // move
    let mut queue_4 = vec![]; // upload
    let mut queue_5 = vec![]; // delete

    let mut src_content_table = build_content_hash_table(src_tree);
    // let dst_path_table = build_path_hash_table(&dst_tree);
    let src_path_table = build_path_hash_table(&src_tree);

    let mut to_move = HashMap::new();
    let mut to_del = HashMap::new();

    // Move file & directories + delete files
    for dst_node in dst_tree {
        // src and dst share the same file/directory
        if let Some(src_node) = src_content_table.remove(&dst_node.content_hash) {
            // only the path is different -> move
            if dst_node.path != src_node.path {
                to_move.insert(
                    dst_node.path.clone(),
                    (
                        Task::Move {
                            from: dst_node.path.clone(),
                            to: src_node.path.clone(),
                        },
                        dst_node.is_file(),
                    ),
                );
            }
            continue;
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
    for new in src_content_table.values() {
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
                        if let Task::Move { from, to } = task {
                            println!(">> {:?} {:?}", from, to);
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
    queue_3 = to_move.into_iter().map(|(_, (task, _))| task).collect();
    queue_3 = dedup_move_tasks(queue_3);

    // Put the remaining delete task to queue 5
    // Make sure that we don't delete any new files / dirs
    // and that we don't delete nested files / dirs that are already deleted in queue1
    queue_5 = to_del
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

    vec![queue_0, queue_1, queue_2, queue_3, queue_4, queue_5]
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

async fn process_move(
    fs: impl FileSystem,
    from: impl AsRef<Path> + Debug,
    to: impl AsRef<Path> + Debug,
) -> io::Result<()> {
    info!("Start moving from {:?} to {:?}", from, to);
    let res = fs.mv(&from, &to).await;
    if res.is_err() {
        error!("Error moving from {:?} to {:?}", from, to);
    } else {
        info!("Done moving from {:?} to {:?}", from, to);
    }
    res
}

async fn process_upload(
    src_fs: impl FileSystem,
    dst_fs: impl FileSystem,
    path: impl AsRef<Path> + Debug,
) -> io::Result<()> {
    info!("Start uploading to {:?}", path);
    let content = src_fs.read(&path).await?;
    let res = dst_fs.write(&path, content).await;
    if res.is_err() {
        error!("Error uploading to {:?}", path);
    } else {
        info!("Done uploading to {:?}", path);
    }
    res
}

async fn process_create_dir(fs: impl FileSystem, path: impl AsRef<Path> + Debug) -> io::Result<()> {
    info!("Start creating dir to {:?}", path);
    let res = fs.mkdir(&path).await;
    if res.is_err() {
        error!("Error creating dir {:?}", path);
    } else {
        info!("Done creating dir {:?}", path);
    }
    res
}

async fn process_delete(fs: impl FileSystem, path: impl AsRef<Path> + Debug) -> io::Result<()> {
    info!("Start deleting {:?}", path);
    let res = fs.rm(&path).await;
    if res.is_err() {
        error!("Error deleting {:?}", path);
    } else {
        info!("Done deleting {:?}", path);
    }
    res
}

pub async fn process_tasks(
    src_fs: impl FileSystem,
    dst_fs: impl FileSystem,
    queues: &Vec<Vec<Task>>,
) -> io::Result<()> {
    for queue in queues {
        let futures = queue.iter().map(|task: &Task| {
            let dst_fs = dst_fs.clone();
            let box_future: Pin<Box<dyn Future<Output = io::Result<()>>>> = match task {
                Task::Move { from, to } => Box::pin(process_move(dst_fs, from.clone(), to.clone())),
                Task::Upload { path } => {
                    Box::pin(process_upload(src_fs.clone(), dst_fs, path.clone()))
                }
                Task::CreateDir { path } => Box::pin(process_create_dir(dst_fs, path.clone())),
                Task::Delete { path } => Box::pin(process_delete(dst_fs, path.clone())),
            };
            box_future
        });
        try_join_all(futures).await?;
    }
    Ok(())
}
