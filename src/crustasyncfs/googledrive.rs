use std::fmt::{Debug, Display};
use std::path::{Path, PathBuf};

use anyhow::Result;
use log::info;
use tokio::io;
use url::Url;

use crate::cli::CLIOption;
use crate::crustasyncfs::base::{FileSystem, Node};
use crate::oauth::{AuthToken, OAuthPublicClient};

// Google client id for public client
const GOOGLE_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
const GOOGLE_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

const CONFIG_FILE_NAME: &str = "google_drive.json";

#[derive(Debug, Clone)]
pub struct GoogleDriveFileSystem {
    auth_token: AuthToken,
}

impl GoogleDriveFileSystem {
    pub async fn new(opt: &CLIOption) -> Result<Self> {
        let mut gd_file = opt.config_dir.clone();
        gd_file.push(CONFIG_FILE_NAME);
        
        let auth_token = match AuthToken::from_file(&gd_file).await {
            Ok(mut token) => {
                if token.is_expired() {
                    Self::auth_client()?.refresh_token(&mut token).await?;
                    Self::save_token(&token, gd_file).await?;
                }
                token
            }
            Err(e) => {
                info!("Cannot find google drive credentials: {}", e);
                let token = Self::auth_client()?.new_auth_token().await?;
                Self::save_token(&token, gd_file).await?;
                token
            }
        };

        Ok(Self { auth_token })
    }

    fn auth_client() -> Result<OAuthPublicClient> {
        Ok(OAuthPublicClient::new(
            GOOGLE_CLIENT_ID,
            GOOGLE_CLIENT_SECRET,
            Url::parse(GOOGLE_AUTH_URL)?,
            Url::parse(GOOGLE_TOKEN_URL)?,
        )?
            .add_scope("https://www.googleapis.com/auth/drive")
            .add_scope("https://www.googleapis.com/auth/drive.metadata")
            .add_scope("https://www.googleapis.com/auth/userinfo.email"))
    }

    async fn save_token(token: &AuthToken, path: impl AsRef<Path> + Debug) -> Result<()> {
        info!("Saving token to {:?}", path);
        token.to_file(path).await?;
        Ok(())
    }
}

impl FileSystem for GoogleDriveFileSystem {
    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        todo!()
    }

    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        todo!()
    }

    async fn mkdir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn rm(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn mv(&self, src: impl AsRef<Path>, dest: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn build_tree(&self) -> io::Result<Node> {
        todo!()
    }
}
