use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json as serde_lib;
use std::path::Path;
use tokio::io;

#[derive(Debug, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Directory,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Node {
    pub node_type: NodeType,
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub content_hash: [u8; 20],
    pub children: Vec<Node>, // TODO box node?
}

impl Node {
    pub fn is_file(&self) -> bool {
        match self.node_type {
            NodeType::File => true,
            NodeType::Directory => false,
        }
    }

    pub fn is_dir(&self) -> bool {
        match self.node_type {
            NodeType::File => false,
            NodeType::Directory => true,
        }
    }
}

pub trait FileSystem {
    const CRUSTASYNC_CONFIG_FILE: &'static str = ".crustasync";

    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()>;

    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>>;

    async fn mkdir(&self, path: impl AsRef<Path>) -> io::Result<()>;

    async fn rm(&self, path: impl AsRef<Path>) -> io::Result<()>;

    async fn mv(&self, src: impl AsRef<Path>, dest: impl AsRef<Path>) -> io::Result<()>;

    async fn build_tree(&self) -> io::Result<Node>;

    async fn sync_fs_to_file(&self) -> io::Result<()> {
        let tree = self.build_tree().await?;
        let serialized = serde_lib::to_string(&tree).unwrap().into_bytes();
        self.write(Self::CRUSTASYNC_CONFIG_FILE, serialized).await?;
        Ok(())
    }

    async fn read_fs_from_file(&self) -> io::Result<Node> {
        let content = self.read(Self::CRUSTASYNC_CONFIG_FILE).await?;
        let json_str = String::from_utf8(content).unwrap();
        let tree: Node = serde_lib::from_str(&json_str).unwrap();
        Ok(tree)
    }
}
