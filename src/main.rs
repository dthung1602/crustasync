mod crustasyncfs;
mod diff;

use crate::diff::Task;
use crustasyncfs::base::{FileSystem, Node};
use crustasyncfs::local::LocalFileSystem;
use tokio::io;

/** Test data
.
├── a.txt
├── b.xml
├── c.md
└── d
    ├── abc
    ├── another
    │   ├── file.txt
    │   └── nested
    ├── deeply
    │   └── nested
    │       └── folder
    │           └── file
    ├── def
    └── xyz
 */

#[tokio::main]
async fn main() -> io::Result<()> {
    let foo_fs = LocalFileSystem::new("foo").await?;
    let foo_tree = foo_fs.build_tree().await?;
    // print_tree(&foo_tree);

    let bar_fs = LocalFileSystem::new("bar").await?;
    let bar_tree = bar_fs.build_tree().await?;

    let task_queues = diff::build_task_queue(&foo_tree, &bar_tree);
    print_task_queues(task_queues);

    return Ok(());
}

fn print_task_queues(queues: Vec<Vec<Task>>) {
    for (i, queue) in queues.iter().enumerate() {
        println!("PRIORITY QUEUE {i}:");
        for task in queue {
            println!(" {:?}", task)
        }
        println!("\n");
    }
}

fn print_tree(node: &Node) {
    println!("TREE:");
    print_node_with_level(node, 0);
    print!("\n");
}

fn print_node_with_level(node: &Node, level: usize) {
    let padding = ' '.to_string().repeat(level * 4);
    println!("{}{}", padding, node.name);

    if node.is_dir() {
        let level = level + 1;
        for child in &node.children {
            print_node_with_level(child, level)
        }
    }
}
