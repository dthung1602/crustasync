use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};
use tokio::fs;
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
    let root = local_fs.build_tree().await?;
    print_tree(&Node::Directory(root));
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

#[derive(Debug)]
enum Node {
    File(File),
    Directory(Directory),
}

#[derive(Debug)]
struct File {
    name: String,
    updated_at: DateTime<Utc>,
    content_hash: Vec<u8>, // TODO reconsider
}

#[derive(Debug)]
struct Directory {
    name: String,
    updated_at: DateTime<Utc>,
    content_hash: Vec<u8>, // TODO reconsider
    children: Vec<Node>,   // TODO box node?
}

type Content = Vec<u8>; // TODO stream?, buffer?

pub trait FileSystem {
    async fn write(&self, path: String, content: Content) -> io::Result<()>;
    async fn read(&self, path: String) -> io::Result<Content>;
    async fn mkdir(&self, path: String) -> io::Result<()>;
    async fn rm(&self, path: String) -> io::Result<()>;
    async fn build_tree(&self) -> io::Result<Directory>;
    async fn sync_tree(&self) -> io::Result<()>;
}

#[derive(Debug)]
struct LocalFileSystem {
    root_dir: String,
}

impl LocalFileSystem {
    async fn new(root_dir: String) -> io::Result<LocalFileSystem> {
        let absolute_path = fs::canonicalize(Path::new(&root_dir)).await?;
        let local_fs = LocalFileSystem { root_dir: absolute_path.to_str().unwrap().to_string() };
        Ok(local_fs)
    }
}

impl FileSystem for LocalFileSystem {
    async fn write(&self, path: String, content: Content) -> io::Result<()> {
        let mut path_buf = Path::new(&path).to_path_buf();
        path_buf.pop();
        fs::create_dir_all(path_buf).await?;
        fs::write(path, content).await
    }

    async fn read(&self, path: String) -> io::Result<Content> {
        fs::read(path).await
    }

    async fn mkdir(&self, path: String) -> io::Result<()> {
        fs::create_dir_all(path).await
    }

    async fn rm(&self, path: String) -> io::Result<()> {
        let meta = fs::metadata(&path).await?;
        if meta.is_dir() {
            fs::remove_dir_all(&path).await
        } else {
            fs::remove_file(&path).await
        }
    }

    async fn build_tree(&self) -> io::Result<Directory> {
        let path_buf = PathBuf::from(&self.root_dir);
        let root = self.build_node(&path_buf).await?;

        match root {
            Node::File(_) => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "root path is not a directory",
            )),
            Node::Directory(dir) => Ok(dir),
        }
    }

    async fn sync_tree(&self) -> io::Result<()> {
        todo!()
    }
}

impl LocalFileSystem {
    async fn build_node(&self, path: &PathBuf) -> io::Result<Node> {
        let meta = fs::metadata(path).await?;
        let updated_at = DateTime::from(meta.modified().unwrap());

        let name = String::from(path.file_name().unwrap().to_str().unwrap());

        if meta.is_dir() {
            let mut result = fs::read_dir(path).await?;
            let mut children = vec![];

            while let Some(entry) = result.next_entry().await? {
                let entry_path = entry.path();
                let node = Box::pin(self.build_node(&entry_path)).await?;
                children.push(node);
            }

            let mut hasher = Sha1::new();

            children.sort_by_key(|node| match node {
                Node::File(file) => file.name.clone().to_lowercase(),
                Node::Directory(dir) => dir.name.clone().to_lowercase(),
            });

            children.iter().for_each(|node| match node {
                Node::File(file) => {
                    let filename = file.name.as_bytes();
                    hasher.update(filename);
                    hasher.update(&file.content_hash);
                }
                Node::Directory(dir) => {
                    let filename = dir.name.as_bytes();
                    hasher.update(filename);
                    hasher.update(&dir.content_hash);
                }
            });

            return Ok(Node::Directory(Directory {
                name,
                updated_at,
                content_hash: hasher.finalize().to_vec(),
                children,
            }));
        }

        // TODO read file as stream
        let content = fs::read(path).await?;
        let mut hasher = Sha1::new();
        hasher.update(content);
        // TODO anything other than vecu8?
        let content_hash = hasher.finalize()[..].to_vec();
        Ok(Node::File(File {
            name,
            updated_at,
            content_hash,
        }))
    }
}
