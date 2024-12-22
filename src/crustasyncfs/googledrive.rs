use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::iter::zip;
use std::path;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use itertools::Itertools;
use log::{debug, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client as ReqwestClient;
use serde::{Deserialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;

use crate::cli::CLIOption;
use crate::crustasyncfs::base::{ContentHash, FileSystem, Node, NodeType};
use crate::oauth::{AuthToken, OAuthPublicClient};

// Google client id for public client
const GOOGLE_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
const GOOGLE_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

const GOOGLE_DRIVE_API_URL: &str = "https://www.googleapis.com/drive/v3";
const GOOGLE_DRIVE_LS_PAGE_SIZE: &str = "10";

const GOOGLE_DRIVE_FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";

const CONFIG_FILE_NAME: &str = "google_drive.json";

#[derive(Debug, Clone)]
pub struct GoogleDriveFileSystem {
    auth_token: AuthToken,
    http_client: ReqwestClient,
    root_dir: PathBuf,
    pub(crate) path_to_meta: HashMap<PathBuf, GDFile>, // TODO temp pub for debug
}

impl GoogleDriveFileSystem {
    pub async fn new(opt: &CLIOption, root_dir: impl AsRef<Path>) -> Result<Self> {
        let mut gd_file = opt.config_dir.clone();
        gd_file.push(CONFIG_FILE_NAME);

        let auth_token = match AuthToken::from_file(&gd_file).await {
            Ok(mut token) => {
                if token.is_expired() {
                    token = Self::auth_client()?.refresh_token(&mut token).await?;
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

        let http_client = reqwest::Client::new();

        Ok(Self {
            auth_token,
            http_client,
            root_dir: root_dir.as_ref().to_path_buf(),
            path_to_meta: HashMap::default(),
        })
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

    async fn auth_header(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", self.auth_token.access_token);
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&bearer)?);
        Ok(headers)
    }

    async fn build_node(
        &self,
        node_id: &str,
        parent_path: impl AsRef<Path>,
        is_root: bool,
        path_to_meta: Arc<Mutex<HashMap<PathBuf, GDFile>>>,
    ) -> Result<Node> {
        let meta = self.metadata(node_id).await?;

        let path = if is_root {
            PathBuf::from("")
        } else {
            parent_path.as_ref().join(&meta.name)
        };

        path_to_meta.lock().await.insert(path.clone(), meta.clone());

        // handle directory
        if meta.is_dir() {
            let children = self.ls(node_id).await?;

            let futures: Vec<_> = children
                .into_iter()
                .filter(|gd_file| !(is_root && gd_file.name == Self::CRUSTASYNC_CONFIG_FILE))
                .map(|gd_file| async {
                    if gd_file.is_dir() {
                        let path_to_meta = path_to_meta.clone();
                        Box::pin(self.build_node(&gd_file.id, &path, false, path_to_meta)).await
                    } else {
                        let child_path = path.join(&gd_file.name);
                        let hash = gd_file.sha256_checksum.as_deref().unwrap();
                        let content_hash: ContentHash = hex::decode(hash)?.try_into().unwrap();
                        let node = Node {
                            node_type: NodeType::File,
                            name: gd_file.name.clone(),
                            path: child_path.clone(),
                            updated_at: gd_file.modified_time.clone(),
                            content_hash,
                            children: vec![],
                        };
                        path_to_meta.lock().await.insert(child_path, gd_file);
                        Ok(node)
                    }
                })
                .collect();

            let mut children = vec![];
            for res in join_all(futures).await {
                match res {
                    Ok(node) => children.push(node),
                    Err(e) => return Err(e),
                }
            }

            let mut hasher = Sha256::new();

            children.sort_by_key(|node| node.name.clone().to_lowercase());

            children.iter().for_each(|node| {
                let filename = node.name.as_bytes();
                hasher.update(filename);
                hasher.update(&node.content_hash);
            });

            let node = Node {
                node_type: NodeType::Directory,
                name: meta.name,
                path,
                updated_at: meta.modified_time,
                content_hash: hasher.finalize().into(),
                children,
            };
            return Ok(node);
        }

        // handle file
        let hash = meta.sha256_checksum.unwrap();
        let content_hash: ContentHash = hex::decode(hash)?.try_into().unwrap();
        let node = Node {
            node_type: NodeType::File,
            name: meta.name,
            path: path.clone(),
            updated_at: meta.modified_time,
            content_hash,
            children: vec![],
        };
        Ok(node)
    }

    async fn metadata(&self, file_id: &str) -> Result<GDFile> {
        let headers = self.auth_header().await.expect("Cannot build headers");
        let query = [("fields", "id, name, mimeType, modifiedTime")];
        Ok(self
            .http_client
            .get(format!("{GOOGLE_DRIVE_API_URL}/files/{file_id}"))
            .headers(headers.clone())
            .query(&query)
            .send()
            .await?
            .json()
            .await?)
    }

    async fn ls(&self, directory_id: &str) -> Result<Vec<GDFile>> {
        debug!("Listing files drives in {directory_id}");

        let headers = self.auth_header().await.expect("Cannot build headers");
        let mut query = vec![
            ("orderBy", "name".to_string()),
            ("pageSize", GOOGLE_DRIVE_LS_PAGE_SIZE.to_string()),
            ("q", Self::gd_query(directory_id, None::<&str>)),
            (
                "fields",
                "nextPageToken, files(id, name, mimeType, modifiedTime, sha256Checksum)"
                    .to_string(),
            ),
        ];

        let mut res = self.do_ls_req(&headers, &query).await?;
        let mut files = res.files;
        debug!("Found {} files in {}", files.len(), directory_id);

        while let Some(next_page_token) = res.next_page_token {
            debug!("Next page token in {}: {}", directory_id, next_page_token);
            query.push(("pageToken", next_page_token.clone()));
            res = self.do_ls_req(&headers, &query).await?;
            debug!("Found {} files in {}", res.files.len(), directory_id);
            files.extend(res.files);
            query.pop();
        }

        debug!("Files in {directory_id}: {files:#?}");
        Ok(files)
    }

    async fn do_ls_req(&self, headers: &HeaderMap, query: &[(&str, String)]) -> Result<GDResp> {
        let res = self
            .http_client
            .get(format!("{GOOGLE_DRIVE_API_URL}/files"))
            .headers(headers.clone())
            .query(&query)
            .send()
            .await?;
        debug!("Got response status: {}", res.status());

        Ok(res
            .json()
            .await
            .expect("Cannot deserialize JSON response to /files"))
    }

    fn gd_query(parent_id: impl ToString, file_name: Option<impl ToString>) -> String {
        let mut query_parts = vec![];

        let escaped_pid = Self::escape_gd_query(parent_id);
        let pid_query = format!("'{}' in parents", escaped_pid);
        query_parts.push(pid_query);

        if let Some(file_name) = file_name {
            let escaped_file_name = Self::escape_gd_query(file_name);
            let file_name_query = format!("name = '{}'", escaped_file_name);
            query_parts.push(file_name_query);
        }

        query_parts.join(" and ")
    }

    #[inline]
    fn escape_gd_query(s: impl ToString) -> String {
        s.to_string().replace("\\", "\\\\").replace("'", "\\'")
    }

    async fn get_root_dir_id(&self) -> Result<String> {
        let root_dir = OsStr::new(path::MAIN_SEPARATOR_STR);
        let mut parent_dir_id = "root".to_string();
        for dir_name in self.root_dir.iter() {
            if dir_name != root_dir {
                parent_dir_id = self
                    .get_child_dir_id(&parent_dir_id, dir_name.to_str().unwrap())
                    .await?;
            };
        }

        Ok(parent_dir_id)
    }

    async fn get_child_dir_id(&self, parent_dir_id: &str, child_name: &str) -> Result<String> {
        let headers = self.auth_header().await.expect("Cannot build header");
        let query = vec![
            ("q", Self::gd_query(parent_dir_id, Some(child_name))),
            (
                "fields",
                "nextPageToken, files(id, name, mimeType, modifiedTime, sha256Checksum)"
                    .to_string(),
            ),
        ];

        let res = self.do_ls_req(&headers, &query).await?;

        if let Some(file) = res.files.first() {
            Ok(file.id.clone())
        } else {
            Err(anyhow!("No files found"))
        }
    }

    async fn create_file(
        &mut self,
        path: impl AsRef<Path>,
        content: impl AsRef<[u8]>,
    ) -> Result<()> {
        todo!()
    }

    async fn update_file(&self, file_meta: &GDFile, content: impl AsRef<[u8]>) -> Result<()> {
        todo!()
    }
}

impl FileSystem for GoogleDriveFileSystem {
    async fn write(&mut self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()> {
        let pb = path.as_ref().to_path_buf();
        if let Some(file_meta) = self.path_to_meta.get(&pb) {
            self.update_file(file_meta, content).await
        } else {
            self.create_file(path, content).await
        }
    }

    async fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let pb = path.as_ref().to_path_buf();
        let file_meta = self
            .path_to_meta
            .get(&pb)
            .expect("Cannot find file id of path");
        debug!("Reading file {:?}", file_meta);

        let url = format!("{}/files/{}", GOOGLE_DRIVE_API_URL, file_meta.id);
        let query = (
            ("alt", "media"),
            ("acknowledgeAbuse", "true"),
            ("supportsAllDrives", "true"),
        );
        let headers = self.auth_header().await.expect("Cannot build headers");

        let response = self
            .http_client
            .get(url)
            .headers(headers)
            .query(&query)
            .send()
            .await
            .expect("Cannot download file")
            .bytes()
            .await?;
        debug!("Downloaded file size: {}", response.len());
        Ok(response.into())
    }

    async fn mkdir(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let parent_path = path.parent().unwrap();
        let parent_child_pairs = zip(parent_path.ancestors(), path.ancestors())
            .collect_vec()
            .into_iter()
            .rev();

        for (parent, child) in parent_child_pairs {
            debug!("Make dir {child:?} with parent {parent:?}");

            let Some(parent_meta) = self.path_to_meta.get(parent) else {
                return Err(anyhow!("Cannot find parent meta data {parent:?}"));
            };

            debug!("Found parent meta {parent_meta:?}");

            if let Some(child_meta) = self.path_to_meta.get(child) {
                if child_meta.is_dir() {
                    debug!("Directory {child:?} already exists");
                    continue;
                }
                return Err(anyhow!(format!(
                    "{child:?} already exists and is not a directory"
                )));
            }

            let name = child.file_name().unwrap().to_str().unwrap();
            let body = json!({
                "name": name,
                "parents": [parent_meta.id.as_str()],
                "mimeType": GOOGLE_DRIVE_FOLDER_MIME_TYPE
            });
            debug!("BODY: {body:?}");
            let url = format!("{GOOGLE_DRIVE_API_URL}/files");
            let headers = self.auth_header().await.expect("Cannot build headers");
            let query = [("fields", "id, name, mimeType, modifiedTime")];
            let response = self
                .http_client
                .post(url)
                .headers(headers)
                .query(&query)
                .json(&body)
                .send()
                .await?;
            debug!("Got response status: {}", response.status());

            let child_meta = response.json().await?;
            debug!("Got response content {:?}", child_meta);
            self.path_to_meta.insert(child.to_path_buf(), child_meta);
        }

        Ok(())
    }

    async fn rm(&mut self, path: impl AsRef<Path>) -> Result<()> {
        todo!()
    }

    async fn mv(&mut self, src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()> {
        todo!()
    }

    async fn build_tree(&mut self) -> Result<Node> {
        let root_dir_id = self.get_root_dir_id().await?;
        debug!("Root dir id: {}", root_dir_id);

        // TODO consider make self.path_to_meta to arc mut
        let path_to_meta = Arc::new(Mutex::new(HashMap::new()));
        let node = self
            .build_node(&root_dir_id, "", true, path_to_meta.clone())
            .await?;
        self.path_to_meta = path_to_meta.lock().await.clone();

        match node.node_type {
            NodeType::File => Err(anyhow!("root is not a directory")),
            NodeType::Directory => Ok(node),
        }
    }
}

// https://developers.google.com/drive/api/guides/search-files
// https://developers.google.com/drive/api/reference/rest/v2/files/list

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GDFile {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(rename = "mimeType")]
    pub(crate) mime_type: String,
    #[serde(rename = "modifiedTime")]
    pub(crate) modified_time: DateTime<Utc>,
    #[serde(rename = "sha256Checksum")]
    pub(crate) sha256_checksum: Option<String>,
    #[serde(rename = "webContentLink")]
    pub(crate) download_url: Option<String>,
}

impl GDFile {
    fn is_dir(&self) -> bool {
        self.mime_type == GOOGLE_DRIVE_FOLDER_MIME_TYPE
    }
}

#[derive(Debug, Deserialize)]
struct GDResp {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    files: Vec<GDFile>,
}
