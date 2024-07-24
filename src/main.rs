mod crustasyncfs;

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
    let local_fs = LocalFileSystem::new(".".to_string()).await?;
    local_fs.sync_fs_to_file().await?;
    let root = local_fs.build_tree().await?;
    print_tree(&Node::Directory(root));
    let root = local_fs.read_fs_from_file().await?;
    print_tree(&Node::Directory(root));
    local_fs.mkdir("new_folder/net/www".to_string()).await?;
    local_fs
        .write(
            "new_folder/net/somefile".to_string(),
            "this is a text".to_string().into_bytes(),
        )
        .await?;
    let content = local_fs.read("new_folder/net/somefile".to_string()).await?;
    let content = String::from_utf8(content).unwrap();
    println!(">>> CONTENT: {content}");
    return Ok(());
}

fn print_tree(node: &Node) {
    print_node_with_level(node, 0)
}

fn print_node_with_level(node: &Node, level: usize) {
    let padding = ' '.to_string().repeat(level * 4);
    match node {
        Node::File(file) => {
            println!("{}{}", padding, file.name);
        }
        Node::Directory(dir) => {
            println!("{}{}", padding, dir.name);
            let level = level + 1;
            for child in &dir.children {
                print_node_with_level(child, level)
            }
        }
    }
}
