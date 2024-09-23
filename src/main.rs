mod crustasyncfs;
mod diff;

use crate::diff::{process_tasks, Task};
use crustasyncfs::base::{FileSystem, Node};
use crustasyncfs::local::LocalFileSystem;
use env_logger::Env;
use hex;
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
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let foo_fs = LocalFileSystem::new("foo").await?;
    let foo_tree = foo_fs.build_tree().await?;
    print_tree(&foo_tree);

    let bar_fs = LocalFileSystem::new("bar").await?;
    let bar_tree = bar_fs.build_tree().await?;
    print_tree(&bar_tree);

    let task_queues = diff::build_task_queue(&foo_tree, &bar_tree);
    print_task_queues(&task_queues);

    process_tasks(foo_fs, bar_fs, &task_queues).await?;

    Ok(())
}

fn print_task_queues(queues: &Vec<Vec<Task>>) {
    for (i, queue) in queues.iter().enumerate() {
        println!("PRIORITY QUEUE {i}:");
        for task in queue {
            println!(" {:?}", task)
        }
        println!("\n");
    }
}

fn print_tree(node: &Node) {
    println!("\n-----------------\nTREE:");
    print_node_with_level(node, 0);
    print!("\n");
}

const PRINT_LINE_WIDTH: usize = 50;

fn print_node_with_level(node: &Node, level: usize) {
    let left_padding = ' '.to_string().repeat(level * 4);
    let node_name = &node.name;
    let encoded = hex::encode(&node.content_hash[0..4]);
    let mut right_padding_len =
        PRINT_LINE_WIDTH - left_padding.len() - node_name.len() - encoded.len();
    let colored_node_name = if node.is_dir() {
        right_padding_len -= 1;
        format!("*{}", node.name.rgb(12, 255, 50))
    } else {
        node.name.default()
    };
    let right_padding = ' '.to_string().repeat(right_padding_len);
    println!("{left_padding}{colored_node_name}{right_padding}{encoded}");

    if node.is_dir() {
        let level = level + 1;
        for child in &node.children {
            print_node_with_level(child, level)
        }
    }
}

pub trait RGBColorTextExt {
    fn rgb(&self, r: u8, g: u8, b: u8) -> String;
    fn default(&self) -> String;
}

impl RGBColorTextExt for String {
    fn rgb(&self, r: u8, g: u8, b: u8) -> String {
        format!("\x1b[38;2;{r};{g};{b}m{self}")
    }

    fn default(&self) -> String {
        format!("\x1b[39m{self}")
    }
}
