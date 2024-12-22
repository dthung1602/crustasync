use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use chrono::DateTime;
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::crustasyncfs::base::{FileSystem, Node, NodeType};

#[derive(Debug, Clone)]
pub struct LocalFileSystem {
    pub(crate) root_dir: PathBuf,
}

impl FileSystem for LocalFileSystem {
    async fn write(&mut self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()> {
        let path_buf = self.abs_path(path);
        let parent = path_buf.parent().unwrap();
        fs::create_dir_all(parent).await?;
        fs::write(path_buf, content)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path_buf = self.abs_path(path);
        fs::read(path_buf).await.map_err(anyhow::Error::from)
    }

    async fn mkdir(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path_buf = self.abs_path(path);
        fs::create_dir_all(path_buf)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn rm(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path_buf = self.abs_path(path);
        let meta = fs::metadata(&path_buf).await?;
        if meta.is_dir() {
            fs::remove_dir_all(&path_buf)
                .await
                .map_err(anyhow::Error::from)
        } else {
            fs::remove_file(&path_buf)
                .await
                .map_err(anyhow::Error::from)
        }
    }

    async fn mv(&mut self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
        fs::rename(self.abs_path(from), self.abs_path(to)).await?;
        Ok(())
    }

    async fn build_tree(&mut self) -> Result<Node> {
        let root = self.build_node(&self.root_dir, "", true).await?;

        match root.node_type {
            NodeType::File => Err(anyhow!("root is not a directory")),
            NodeType::Directory => Ok(root),
        }
    }
}

impl LocalFileSystem {
    pub async fn new(root_dir: impl AsRef<Path>) -> Result<LocalFileSystem> {
        let absolute_path = fs::canonicalize(root_dir).await?;

        let metadata = fs::metadata(&absolute_path).await?;
        if !metadata.is_dir() {
            return Err(anyhow!("root is not a directory"));
        }

        let local_fs = LocalFileSystem {
            root_dir: absolute_path,
        };
        Ok(local_fs)
    }

    fn abs_path(&self, relative_path: impl AsRef<Path>) -> PathBuf {
        self.root_dir.join(relative_path)
    }

    async fn build_node(
        &self,
        abs_path: impl AsRef<Path>,
        parent_path: impl AsRef<Path>,
        is_root: bool,
    ) -> Result<Node> {
        let meta = fs::metadata(&abs_path).await?;
        let updated_at = DateTime::from(meta.modified()?);
        let name = String::from(abs_path.as_ref().file_name().unwrap().to_str().unwrap());
        let path = if is_root {
            PathBuf::from("")
        } else {
            parent_path.as_ref().join(&name)
        };

        if meta.is_dir() {
            let mut result = fs::read_dir(abs_path).await?;
            let mut children = vec![];

            while let Some(entry) = result.next_entry().await? {
                if is_root && entry.file_name().to_str().unwrap() == Self::CRUSTASYNC_CONFIG_FILE {
                    continue;
                }
                let entry_path = entry.path();
                let node = Box::pin(self.build_node(entry_path, &path, false)).await?;
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
