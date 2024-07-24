use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json as serde_lib;
use tokio::io;

#[derive(Debug, Serialize, Deserialize)]
pub enum Node {
    File(File),
    Directory(Directory),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub content_hash: Vec<u8>, // TODO reconsider
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Directory {
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub content_hash: Vec<u8>, // TODO reconsider
    pub children: Vec<Node>,   // TODO box node?
}

pub type Content = Vec<u8>; // TODO stream?, buffer?

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
