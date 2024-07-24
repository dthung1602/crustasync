use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json as serde_lib;
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
    local_fs.sync_fs_to_file().await?;
    let root = local_fs.build_tree().await?;
    print_tree(&Node::Directory(root));
    let root = local_fs.read_fs_from_file().await?;
    print_tree(&Node::Directory(root));
    local_fs.mkdir("new_folder/net/www".to_string()).await?;
    local_fs.write("new_folder/net/somefile".to_string(), "this is a text".to_string().into_bytes()).await?;
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

#[derive(Debug, Serialize, Deserialize)]
pub enum Node {
    File(File),
    Directory(Directory),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    name: String,
    updated_at: DateTime<Utc>,
    content_hash: Vec<u8>, // TODO reconsider
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Directory {
    name: String,
    updated_at: DateTime<Utc>,
    content_hash: Vec<u8>, // TODO reconsider
    children: Vec<Node>,   // TODO box node?
}

type Content = Vec<u8>; // TODO stream?, buffer?

pub trait FileSystem {
    const CRUSTASYNC_CONFIG_FILE: &'static str = ".crustasync";

    async fn write(&self, path: String, content: Content) -> io::Result<()>;

    async fn read(&self, path: String) -> io::Result<Content>;

    async fn mkdir(&self, path: String) -> io::Result<()>;

    async fn rm(&self, path: String) -> io::Result<()>;

    async fn build_tree(&self) -> io::Result<Directory>;

    async fn sync_fs_to_file(&self) -> io::Result<()> {
        let tree = self.build_tree().await?;
        let serialized = serde_lib::to_string(&tree).unwrap().into_bytes();
        self.write(Self::CRUSTASYNC_CONFIG_FILE.to_string(), serialized)
            .await?;
        Ok(())
    }

    async fn read_fs_from_file(&self) -> io::Result<Directory> {
        let content = self.read(Self::CRUSTASYNC_CONFIG_FILE.to_string()).await?;
        let json_str = String::from_utf8(content).unwrap();
        let tree: Directory = serde_lib::from_str(&json_str).unwrap();
        Ok(tree)
    }
}

#[derive(Debug)]
pub struct LocalFileSystem {
    root_dir: String,
}

impl LocalFileSystem {
    async fn new(root_dir: String) -> io::Result<LocalFileSystem> {
        let absolute_path = fs::canonicalize(Path::new(&root_dir)).await?;
        let local_fs = LocalFileSystem {
            root_dir: absolute_path.to_str().unwrap().to_string(),
        };
        Ok(local_fs)
    }
}

impl FileSystem for LocalFileSystem {
    async fn write(&self, path: String, content: Content) -> io::Result<()> {
        let path_buf = self.absolute_path(&path);
        let parent = path_buf.parent().unwrap();
        fs::create_dir_all(parent).await?;
        fs::write(path_buf, content).await
    }

    async fn read(&self, path: String) -> io::Result<Content> {
        let path_buf = self.absolute_path(&path);
        fs::read(path_buf).await
    }

    async fn mkdir(&self, path: String) -> io::Result<()> {
        let path_buf = self.absolute_path(&path);
        fs::create_dir_all(path_buf).await
    }

    async fn rm(&self, path: String) -> io::Result<()> {
        let path_buf = self.absolute_path(&path);
        let meta = fs::metadata(&path_buf).await?;
        if meta.is_dir() {
            fs::remove_dir_all(&path_buf).await
        } else {
            fs::remove_file(&path_buf).await
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
}

impl LocalFileSystem {
    fn absolute_path(&self, path: &str) -> PathBuf {
        let mut path_buf = PathBuf::from(&self.root_dir);
        path_buf.push(path);
        path_buf
    }

    async fn build_node(&self, path_buf: &PathBuf) -> io::Result<Node> {
        let meta = fs::metadata(path_buf).await?;
        let updated_at = DateTime::from(meta.modified().unwrap());

        let name = String::from(path_buf.file_name().unwrap().to_str().unwrap());

        if meta.is_dir() {
            let mut result = fs::read_dir(path_buf).await?;
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
        let content = fs::read(path_buf).await?;
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
