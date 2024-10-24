use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json as serde_lib;

// ------------------------------
// region Node
// ------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Directory,
}

// SHA256 hash result is 32 bytes
pub type ContentHash = [u8; 32];

#[derive(Debug, Serialize, Deserialize)]
pub struct Node {
    pub node_type: NodeType,
    pub name: String,
    pub path: PathBuf,
    pub updated_at: DateTime<Utc>,
    pub content_hash: ContentHash,
    pub children: Vec<Node>,
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

pub struct NodeIterator<'a> {
    node: &'a Node,
    dequeue: VecDeque<&'a Node>,
}

impl<'a> NodeIterator<'a> {
    fn new(node: &'a Node) -> NodeIterator<'a> {
        NodeIterator {
            node,
            dequeue: VecDeque::from(vec![node]),
        }
    }
}

impl<'a> Iterator for NodeIterator<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<Self::Item> {
        let front = match self.dequeue.pop_front() {
            None => return None,
            Some(front) => front,
        };

        if front.is_dir() {
            for child in &front.children {
                self.dequeue.push_back(child);
            }
        }

        Some(front)
    }
}

impl<'a> IntoIterator for &'a Node {
    type Item = &'a Node;
    type IntoIter = NodeIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        NodeIterator::new(self)
    }
}
// endregion

// ------------------------------
// region FileSystem
// ------------------------------

pub trait FileSystem: Clone {
    const CRUSTASYNC_CONFIG_FILE: &'static str = ".crustasync";

    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()>;

    async fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>>;

    async fn mkdir(&self, path: impl AsRef<Path>) -> Result<()>;

    async fn rm(&self, path: impl AsRef<Path>) -> Result<()>;

    async fn mv(&self, src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()>;

    async fn build_tree(&self) -> Result<Node>;

    async fn sync_fs_to_file(&self) -> Result<()> {
        let tree = self.build_tree().await?;
        let serialized = serde_lib::to_string(&tree)?.into_bytes();
        self.write(Self::CRUSTASYNC_CONFIG_FILE, serialized).await?;
        Ok(())
    }

    async fn read_fs_from_file(&self) -> Result<Node> {
        let content = self.read(Self::CRUSTASYNC_CONFIG_FILE).await?;
        let json_str = String::from_utf8(content)?;
        let tree: Node = serde_lib::from_str(&json_str)?;
        Ok(tree)
    }
}

// endregion
