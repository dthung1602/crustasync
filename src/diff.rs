use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;

use futures::future::{try_join_all, Future};
use tokio::io;

use crate::crustasyncfs::base::{ContentHash, FileSystem, Node};

#[derive(Clone, Debug)]
pub enum Task {
    Move { from: PathBuf, to: PathBuf },
    Upload { path: PathBuf },
    CreateDir { path: PathBuf },
    Delete { path: PathBuf },
}

fn build_hash_table(tree: &Node) -> HashMap<ContentHash, &Node> {
    // TODO files with same content
    let mut table = HashMap::new();
    for node in tree {
        table.insert(node.content_hash, node);
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
    let mut queue_0 = vec![]; // create dir
    let mut queue_1 = vec![]; // upload, move,
    let mut queue_2 = vec![]; // delete

    let mut new_nodes: HashSet<PathBuf> = HashSet::new();

    let mut src_node_table = build_hash_table(src_tree);

    // Move file & directories + delete files
    for dst_node in dst_tree {
        // src and dst share the same file/directory
        if let Some(src_node) = src_node_table.remove(&dst_node.content_hash) {
            // only the path is different -> move
            if dst_node.name != src_node.name {
                queue_1.push(Task::Move {
                    from: src_node.path.clone(),
                    to: dst_node.path.clone(),
                });
                new_nodes.insert(dst_node.path.clone());
            }
            continue;
        };

        // content not found in src tree
        // is file -> delete
        if dst_node.is_file() {
            queue_2.push(Task::Delete {
                path: dst_node.path.clone(),
            })
        }
        // is dir -> leave it there for now, the nested files/directories might be reused
        // will clean up after everything is done
        else if dst_node.path != empty_path {
            // skip dst root dir
            queue_2.push(Task::Delete {
                path: dst_node.path.clone(),
            })
        }
    }

    // Create dir & Upload new files
    for new in src_node_table.values() {
        if new.is_file() {
            queue_1.push(Task::Upload {
                path: new.path.clone(),
            });
        } else if new.path != empty_path {
            // skip dst root dir
            queue_0.push(Task::CreateDir {
                path: new.path.clone(),
            })
        }
        new_nodes.insert(new.path.clone());
    }

    // make sure that parent directories are created first
    queue_0.sort_by_key(|task| {
        if let Task::CreateDir { path } = task {
            String::from(path.to_str().unwrap())
        } else {
            String::new()
        }
    });

    // Do not delete newly created files and directories
    let mut queue_2: Vec<Task> = queue_2
        .iter()
        .filter_map(|task| {
            if let Task::Delete { path } = task {
                if !new_nodes.contains(path) {
                    return Some(task.clone());
                }
            }
            None
        })
        .collect();

    // Make sure to delete all nested node before deleting the parent dir
    queue_2.sort_by_key(|task| {
        let s = if let Task::Delete { path } = task {
            String::from(path.to_str().unwrap())
        } else {
            String::new()
        };
        Reverse(s)
    });

    vec![queue_0, queue_1, queue_2]
}

async fn process_move(
    fs: impl FileSystem,
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> io::Result<()> {
    fs.mv(from, to).await
}

async fn process_upload(
    src_fs: impl FileSystem,
    dst_fs: impl FileSystem,
    path: impl AsRef<Path>,
) -> io::Result<()> {
    let content = src_fs.read(&path).await?;
    dst_fs.write(&path, content).await
}

async fn process_create_dir(fs: impl FileSystem, path: impl AsRef<Path>) -> io::Result<()> {
    fs.mkdir(path).await
}

async fn process_delete(fs: impl FileSystem, path: impl AsRef<Path>) -> io::Result<()> {
    fs.rm(path).await
}

pub async fn process_tasks(
    src_fs: impl FileSystem,
    dst_fs: impl FileSystem,
    queues: Vec<Vec<Task>>,
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
