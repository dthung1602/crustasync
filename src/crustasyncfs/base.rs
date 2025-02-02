use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json as serde_lib;

use crate::error::Result;

// ------------------------------
// region Node
// ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Directory,
}

// SHA256 hash result is 32 bytes
pub type ContentHash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    dequeue: VecDeque<&'a Node>,
}

impl<'a> NodeIterator<'a> {
    fn new(node: &'a Node) -> NodeIterator<'a> {
        NodeIterator {
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

pub const CRUSTASYNC_CONFIG_FILE: &'static str = ".crustasync";

#[async_trait]
// TODO only put some part of fs in arc<lwlock>
pub trait FileSystem {
    async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()>;

    async fn read(&mut self, path: &Path) -> Result<Vec<u8>>;

    async fn mkdir(&mut self, path: &Path) -> Result<()>;

    async fn rm(&mut self, path: &Path) -> Result<()>;

    async fn mv(&mut self, src: &Path, dest: &Path) -> Result<()>;

    async fn build_tree(&mut self) -> Result<Node>;

    async fn sync_fs_to_file(&mut self) -> Result<()> {
        let tree = self.build_tree().await?;
        let serialized = serde_lib::to_string(&tree)?.into_bytes();
        self.write(CRUSTASYNC_CONFIG_FILE.as_ref(), serialized.as_ref())
            .await?;
        Ok(())
    }

    async fn read_fs_from_file(&mut self) -> Result<Node> {
        let content = self.read(CRUSTASYNC_CONFIG_FILE.as_ref()).await?;
        let json_str = String::from_utf8(content)?;
        let tree: Node = serde_lib::from_str(&json_str)?;
        Ok(tree)
    }
}

// endregion
