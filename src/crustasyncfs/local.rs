use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::DateTime;
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::crustasyncfs::base::{FileSystem, Node, NodeType, CRUSTASYNC_CONFIG_FILE};
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct LocalFileSystem {
    pub(crate) root_dir: PathBuf,
}

#[async_trait]
impl FileSystem for LocalFileSystem {
    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path_buf = self.abs_path(path);
        let parent = path_buf.parent().unwrap();
        fs::create_dir_all(parent).await?;
        fs::write(path_buf, content).await?;
        Ok(())
    }

    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path_buf = self.abs_path(path);
        Ok(fs::read(path_buf).await?)
    }

    async fn mkdir(&self, path: &Path) -> Result<()> {
        let path_buf = self.abs_path(path);
        fs::create_dir_all(path_buf).await?;
        Ok(())
    }

    async fn rm(&self, path: &Path) -> Result<()> {
        let path_buf = self.abs_path(path);
        let meta = fs::metadata(&path_buf).await?;
        if meta.is_dir() {
            fs::remove_dir_all(&path_buf).await?
        } else {
            fs::remove_file(&path_buf).await?
        };
        Ok(())
    }

    async fn mv(&self, from: &Path, to: &Path) -> Result<()> {
        fs::rename(self.abs_path(from), self.abs_path(to)).await?;
        Ok(())
    }

    async fn build_tree(&self) -> Result<Node> {
        let root = self.build_node(&self.root_dir, "".as_ref(), true).await?;

        match root.node_type {
            NodeType::File => Err(Error::ExpectDirectory(self.root_dir.clone())),
            NodeType::Directory => Ok(root),
        }
    }
}

impl LocalFileSystem {
    pub async fn new(root_dir: &Path) -> Result<LocalFileSystem> {
        let absolute_path = fs::canonicalize(root_dir).await?;

        let metadata = fs::metadata(&absolute_path).await?;
        if !metadata.is_dir() {
            return Err(Error::ExpectDirectory(absolute_path));
        }

        let local_fs = LocalFileSystem {
            root_dir: absolute_path,
        };
        Ok(local_fs)
    }

    fn abs_path(&self, relative_path: &Path) -> PathBuf {
        self.root_dir.join(relative_path)
    }

    async fn build_node(&self, abs_path: &Path, parent_path: &Path, is_root: bool) -> Result<Node> {
        let meta = fs::metadata(&abs_path).await?;
        let updated_at = DateTime::from(meta.modified()?);
        let name = String::from(abs_path.file_name().unwrap().to_str().unwrap());
        let path = if is_root {
            PathBuf::from("")
        } else {
            parent_path.to_path_buf().join(&name)
        };

        if meta.is_dir() {
            let mut result = fs::read_dir(abs_path).await?;
            let mut children = vec![];

            while let Some(entry) = result.next_entry().await? {
                if is_root && entry.file_name().to_str().unwrap() == CRUSTASYNC_CONFIG_FILE {
                    continue;
                }
                let entry_path = entry.path();
                let node = Box::pin(self.build_node(&entry_path, &path, false)).await?;
                children.push(node);
            }

            let mut hasher = Sha256::new();

            children.sort_by_key(|node| node.name.clone().to_lowercase());

            children.iter().for_each(|node| {
                let filename = node.name.as_bytes();
                hasher.update(filename);
                hasher.update(&node.content_hash);
            });

            return Ok(Node {
                node_type: NodeType::Directory,
                name,
                path,
                updated_at,
                content_hash: hasher.finalize().into(),
                children,
            });
        }

        // TODO read file as stream
        let content = fs::read(abs_path).await?;
        let mut hasher = Sha256::new();
        hasher.update(content);
        let content_hash = hasher.finalize().into();

        Ok(Node {
            node_type: NodeType::File,
            name,
            path,
            updated_at,
            content_hash,
            children: vec![],
        })
    }
}
