use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io;

use crate::crustasyncfs::base::{FileSystem, Node, NodeType};

#[derive(Debug)]
pub struct LocalFileSystem {
    root_dir: String,
}

impl FileSystem for LocalFileSystem {
    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        let path_buf = self.absolute_path(path);
        let parent = path_buf.parent().unwrap();
        fs::create_dir_all(parent).await?;
        fs::write(path_buf, content).await
    }

    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        let path_buf = self.absolute_path(path);
        fs::read(path_buf).await
    }

    async fn mkdir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path_buf = self.absolute_path(path);
        fs::create_dir_all(path_buf).await
    }

    async fn rm(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path_buf = self.absolute_path(path);
        let meta = fs::metadata(&path_buf).await?;
        if meta.is_dir() {
            fs::remove_dir_all(&path_buf).await
        } else {
            fs::remove_file(&path_buf).await
        }
    }

    async fn mv(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        fs::rename(self.absolute_path(from), self.absolute_path(to)).await?;
        Ok(())
    }

    async fn build_tree(&self) -> io::Result<Node> {
        let path_buf = PathBuf::from(&self.root_dir);
        let root = self.build_node(&path_buf).await?;

        match root.node_type {
            NodeType::File => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "root path is not a directory",
            )),
            NodeType::Directory => Ok(root),
        }
    }
}

impl LocalFileSystem {
    pub async fn new(root_dir: impl AsRef<Path>) -> io::Result<LocalFileSystem> {
        let absolute_path = fs::canonicalize(root_dir).await?;

        let metadata = fs::metadata(&absolute_path).await?;
        if !metadata.is_dir() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "root path is not a directory",
            ));
        }

        let local_fs = LocalFileSystem {
            root_dir: absolute_path.to_str().unwrap().to_string(),
        };
        Ok(local_fs)
    }

    fn absolute_path(&self, path: impl AsRef<Path>) -> PathBuf {
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

            children.sort_by_key(|node| node.name.clone().to_lowercase());

            children.iter().for_each(|node| {
                let filename = node.name.as_bytes();
                hasher.update(filename);
                hasher.update(&node.content_hash);
            });

            return Ok(Node {
                node_type: NodeType::Directory,
                name,
                updated_at,
                content_hash: hasher.finalize().into(),
                children,
            });
        }

        // TODO read file as stream
        let content = fs::read(path_buf).await?;
        let mut hasher = Sha1::new();
        hasher.update(content);
        let content_hash = hasher.finalize().into();
        return Ok(Node {
            node_type: NodeType::File,
            name,
            updated_at,
            content_hash,
            children: vec![],
        });
    }
}
