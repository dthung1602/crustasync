use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::iter::zip;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};
use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use itertools::Itertools;
use log::{debug, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;

use crate::cli::CLIOption;
use crate::crustasyncfs::base::{ContentHash, FileSystem, Node, NodeType, CRUSTASYNC_CONFIG_FILE};
use crate::error::{Error, Result};
use crate::oauth::AuthError;
use crate::oauth::{AuthToken, OAuthPublicClient};

// ------------------------------
// region Error
// ------------------------------

pub enum GDError {
    MissingField { field: String },
    InvalidData { field: String, message: String },
    FileNotFound { file: String },
    ParentNotFound { file: String },
    Authentication(AuthError),
}

impl std::error::Error for GDError {}

impl Debug for GDError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self, f)
    }
}

impl std::fmt::Display for GDError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GDError::MissingField { field } => write!(f, "GDError: Missing field {field}"),
            GDError::InvalidData { field, message } => {
                write!(f, "GDError: Invalid data in {field}, {message}")
            }
            GDError::FileNotFound { file } => {
                write!(f, "GDError: File not found {file}")
            }
            GDError::ParentNotFound { file } => {
                write!(f, "GDError: Cannot find parent of {file}")
            }
            GDError::Authentication(error) => std::fmt::Display::fmt(error, f),
        }
    }
}

impl From<AuthError> for GDError {
    fn from(error: AuthError) -> Self {
        GDError::Authentication(error)
    }
}

// endregion

// ------------------------------
// region GDFile
// ------------------------------

// https://developers.google.com/drive/api/guides/search-files
// https://developers.google.com/drive/api/reference/rest/v3/files/list

// TODO pub for debug
#[derive(Debug, Deserialize, Clone)]
pub struct GDFile {
    pub id: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "modifiedTime")]
    pub modified_time: DateTime<Utc>,
    #[serde(rename = "sha256Checksum")]
    pub sha256_checksum: Option<String>,
    #[serde(rename = "webContentLink")]
    pub download_url: Option<String>,
}

impl GDFile {
    fn is_dir(&self) -> bool {
        self.mime_type == GOOGLE_DRIVE_FOLDER_MIME_TYPE
    }

    fn assert_is_dir(&self) -> Result<()> {
        if self.is_dir() {
            Ok(())
        } else {
            Err(Error::ExpectDirectory(PathBuf::from(self.name.clone())))
        }
    }

    fn content_hash(&self) -> Result<ContentHash> {
        let Some(hash) = &self.sha256_checksum else {
            return Err(Error::from(GDError::MissingField {
                field: "sha256_checksum".to_string(),
            }));
        };
        let Ok(content_hash) = hex::decode(hash) else {
            return Err(Error::from(GDError::InvalidData {
                field: "sha256_checksum".to_string(),
                message: "Cannot decode hex value".to_string(),
            }));
        };
        match content_hash.try_into() {
            Ok(hash) => Ok(hash),
            Err(_) => Err(Error::Unknown(anyhow!("Cannot convert hash"))),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GDResp {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    files: Vec<GDFile>,
}

// endregion

// ------------------------------
// region FileSystem
// ------------------------------

// Google client id for public client
const GOOGLE_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
const GOOGLE_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

const GOOGLE_DRIVE_API_URL: &str = "https://www.googleapis.com/drive/v3";
const GOOGLE_DRIVE_UPLOAD_API_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const GOOGLE_DRIVE_LS_PAGE_SIZE: &str = "10";

const GOOGLE_DRIVE_FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";

const CONFIG_FILE_NAME: &str = "google_drive.json";

#[derive(Debug, Clone)]
pub struct GoogleDriveFileSystem {
    auth_token: AuthToken,
    http_client: ReqwestClient,
    root_dir: PathBuf,
    path_to_meta: HashMap<PathBuf, GDFile>,
    tree: Option<Node>, // TODO need to save tree?
}

impl GoogleDriveFileSystem {
    pub async fn new(opt: &CLIOption, root_dir: &Path) -> Result<Self> {
        let mut gd_file = opt.config_dir.clone();
        gd_file.push(CONFIG_FILE_NAME);

        let auth_token = match AuthToken::from_file(&gd_file).await {
            Ok(mut token) => {
                if token.is_expired() {
                    token = Self::auth_client()?
                        .refresh_token(&mut token)
                        .await
                        .map_err(|e| Error::from(GDError::from(e)))?;
                    Self::save_token(&token, &gd_file).await?;
                }
                token
            }
            Err(e) => {
                info!("Cannot find google drive credentials: {}", e);
                let token = Self::auth_client()?
                    .new_auth_token()
                    .await
                    .map_err(|e| Error::from(GDError::from(e)))?;
                Self::save_token(&token, &gd_file).await?;
                token
            }
        };

        let http_client = reqwest::Client::new();

        Ok(Self {
            auth_token,
            http_client,
            root_dir: root_dir.to_path_buf(),
            path_to_meta: HashMap::default(),
            tree: None,
        })
    }

    fn auth_client() -> Result<OAuthPublicClient> {
        let client = OAuthPublicClient::new(
            GOOGLE_CLIENT_ID,
            GOOGLE_CLIENT_SECRET,
            Url::parse(GOOGLE_AUTH_URL).unwrap(),
            Url::parse(GOOGLE_TOKEN_URL).unwrap(),
        );

        match client {
            Ok(client) => Ok(client
                .add_scope("https://www.googleapis.com/auth/drive")
                .add_scope("https://www.googleapis.com/auth/drive.metadata")
                .add_scope("https://www.googleapis.com/auth/userinfo.email")),
            Err(e) => Err(Error::from(GDError::from(e))),
        }
    }

    async fn save_token(token: &AuthToken, path: &Path) -> Result<()> {
        info!("Saving token to {:?}", path);
        token
            .to_file(path)
            .await
            .map_err(|e| GDError::from(e).into())
    }

    async fn auth_header(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", self.auth_token.access_token);
        let header_value = HeaderValue::from_str(&bearer).map_err(|e| GDError::InvalidData {
            field: "Authorizaion header".to_string(),
            message: e.to_string(),
        })?;
        headers.insert(AUTHORIZATION, header_value);
        Ok(headers)
    }

    async fn build_node(
        &self,
        node_id: &str,
        parent_path: &Path,
        is_root: bool,
        path_to_meta: Arc<Mutex<HashMap<PathBuf, GDFile>>>,
    ) -> Result<Node> {
        let meta = self.metadata(node_id).await?;

        let path = if is_root {
            PathBuf::from("")
        } else {
            parent_path.to_path_buf().join(&meta.name)
        };

        path_to_meta.lock().await.insert(path.clone(), meta.clone());

        // handle directory
        if meta.is_dir() {
            let children = self.ls(node_id).await?;

            let futures: Vec<_> = children
                .into_iter()
                .map(|gd_file| async {
                    if gd_file.is_dir() {
                        let path_to_meta = path_to_meta.clone();
                        Box::pin(self.build_node(&gd_file.id, &path, false, path_to_meta)).await
                    } else {
                        let child_path = path.join(&gd_file.name);
                        let content_hash = gd_file.content_hash()?;
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
                    Ok(node) => {
                        if !(is_root && node.name == CRUSTASYNC_CONFIG_FILE) {
                            // still update .crustasync config id, path, but do not include in tree
                            children.push(node)
                        }
                    }
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
        let content_hash = meta.content_hash()?;
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
        let headers = self.auth_header().await?;
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

        let headers = self.auth_header().await?;
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

        Ok(res.json().await?)
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
        let root_dir = OsStr::new(MAIN_SEPARATOR_STR);
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
        let headers = self.auth_header().await?;
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
            Err(Error::from(GDError::FileNotFound {
                file: child_name.to_string(),
            }))
        }
    }

    pub async fn init(&mut self) -> Result<()> {
        if self.tree.is_none() {
            debug!("Initializing gd file system");
            self.tree = Some(self.build_tree().await?)
        }
        Ok(())
    }
}

#[async_trait]
impl FileSystem for GoogleDriveFileSystem {
    async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()> {
        self.init().await?;

        // check parent is dir
        let Some(path_parent) = path.parent() else {
            return Err(Error::from(GDError::ParentNotFound {
                file: path.display().to_string(),
            }));
        };
        let parent_pb = path_parent.to_path_buf();
        let Some(parent_meta) = self.path_to_meta.get(&parent_pb) else {
            return Err(Error::from(GDError::ParentNotFound {
                file: path.display().to_string(),
            }));
        };
        parent_meta.assert_is_dir()?;

        // decide whether to create or update
        let gd_meta = self.path_to_meta.get(path);
        let req_builder = if let Some(gd_meta) = gd_meta {
            debug!("Updating file at {}", path.display());
            self.http_client
                .patch(format!("{}/{}", GOOGLE_DRIVE_UPLOAD_API_URL, gd_meta.id))
        } else {
            debug!("Creating file at {}", path.display());
            let name = path.file_name().unwrap().to_str().unwrap();
            let body = json!({
                "name": name,
                "parents": [parent_meta.id.as_str()],
            });
            self.http_client
                .post(GOOGLE_DRIVE_UPLOAD_API_URL)
                .json(&body)
        };

        // make first request to acquire the upload session url
        let headers = self.auth_header().await?;
        let query = [("uploadType", "resumable")];
        let res = req_builder
            .headers(headers)
            .query(&query)
            .send()
            .await?
            .error_for_status()?;
        let res_headers = res.headers();
        let Some(location) = res_headers.get("location") else {
            return Err(Error::from(GDError::MissingField {
                field: "HTTP redirect location header".to_string(),
            }));
        };
        debug!("Upload url {:?}", location);

        // actually upload the file
        // TODO upload in chunk for large file
        let content = Vec::from(content.as_ref());
        let content_len = content.len().to_string();
        let header_value = HeaderValue::from_str(&content_len).map_err(|e| Error::Unknown(anyhow!(e)))?;
        let mut headers = self.auth_header().await?;
        headers.append("content-length", header_value);
        let file_meta: serde_json::Value = self
            .http_client
            .put(location.to_str().unwrap())
            .headers(headers)
            .body(content)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        debug!("Upload response: {:?}", file_meta);

        // file_meta doesn't contain all fields we need
        // request again for metadata
        let file_id = file_meta.get("id").unwrap().as_str().unwrap();
        let file_meta = self.metadata(file_id).await?;
        self.path_to_meta.insert(path.to_path_buf(), file_meta);

        Ok(())
    }

    async fn read(&mut self, path: &Path) -> Result<Vec<u8>> {
        self.init().await?;

        let pb = path.to_path_buf();
        let Some(file_meta) = self.path_to_meta.get(&pb) else {
            return Err(Error::from(GDError::FileNotFound {
                file: pb.to_string_lossy().to_string(),
            }));
        };
        debug!("Reading file {:?}", file_meta);

        let url = format!("{}/files/{}", GOOGLE_DRIVE_API_URL, file_meta.id);
        let query = (
            ("alt", "media"),
            ("acknowledgeAbuse", "true"),
            ("supportsAllDrives", "true"),
        );
        let headers = self.auth_header().await?;

        let response = self
            .http_client
            .get(url)
            .headers(headers)
            .query(&query)
            .send()
            .await?
            .bytes()
            .await?;
        debug!("Downloaded file size: {}", response.len());
        Ok(response.into())
    }

    async fn mkdir(&mut self, path: &Path) -> Result<()> {
        self.init().await?;

        let parent_path = path.parent().unwrap();
        let parent_child_pairs = zip(parent_path.ancestors(), path.ancestors())
            .collect_vec()
            .into_iter()
            .rev();

        for (parent, child) in parent_child_pairs {
            debug!("Make dir {child:?} with parent {parent:?}");

            let Some(parent_meta) = self.path_to_meta.get(parent) else {
                return Err(Error::from(GDError::ParentNotFound {
                    file: parent.display().to_string(),
                }));
            };

            debug!("Found parent meta {parent_meta:?}");

            if let Some(child_meta) = self.path_to_meta.get(child) {
                if child_meta.is_dir() {
                    debug!("Directory {child:?} already exists");
                    continue;
                }
                return Err(Error::ExpectDirectory(child.to_path_buf()));
            }

            let name = child.file_name().unwrap().to_str().unwrap();
            let body = json!({
                "name": name,
                "parents": [parent_meta.id.as_str()],
                "mimeType": GOOGLE_DRIVE_FOLDER_MIME_TYPE
            });
            debug!("BODY: {body:?}");
            let url = format!("{GOOGLE_DRIVE_API_URL}/files");
            let headers = self.auth_header().await?;
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

    async fn rm(&mut self, path: &Path) -> Result<()> {
        self.init().await?;

        let Some(GDFile { id, .. }) = self.path_to_meta.get(path) else {
            return Err(Error::from(GDError::FileNotFound {
                file: path.display().to_string(),
            }));
        };
        debug!("Removing file {path:?} with id {id:?}");

        let url = format!("{GOOGLE_DRIVE_API_URL}/files/{id}");
        let headers = self.auth_header().await?;
        self.http_client
            .delete(url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?;
        self.path_to_meta.remove(path);
        Ok(())
    }

    async fn mv(&mut self, src: &Path, dest: &Path) -> Result<()> {
        self.init().await?;

        if self.path_to_meta.contains_key(dest) {
            debug!("File/folder at {dest:?} exists. Removing");
            self.rm(dest).await?;
        }
        let Some(src_meta) = self.path_to_meta.get(src) else {
            return Err(Error::from(GDError::FileNotFound {
                file: src.to_string_lossy().to_string(),
            }));
        };

        debug!("Moving file {src:?} to {dest:?}");

        let Some(src_parent) = src.parent() else {
            return Err(Error::from(GDError::ParentNotFound {
                file: src.display().to_string(),
            }));
        };
        let Some(src_parent_meta) = self.path_to_meta.get(src_parent) else {
            return Err(Error::from(GDError::FileNotFound {
                file: src_parent.to_string_lossy().to_string(),
            }));
        };
        debug!("Src parent folder {src_parent:?} {src_parent_meta:?}");

        let Some(dest_parent) = dest.parent() else {
            return Err(Error::from(GDError::ParentNotFound {
                file: dest.display().to_string(),
            }));
        };
        let Some(dest_parent_meta) = self.path_to_meta.get(dest_parent) else {
            return Err(Error::from(GDError::FileNotFound {
                file: dest_parent.to_string_lossy().to_string(),
            }));
        };
        debug!("Dest parent folder {dest_parent:?} {dest_parent_meta:?}");
        if !dest_parent_meta.is_dir() {
            return Err(Error::ExpectDirectory(dest_parent.to_path_buf()));
        }

        let url = format!("{}/files/{}", GOOGLE_DRIVE_API_URL, src_meta.id);
        let headers = self.auth_header().await?;
        let query = [
            ("fields", "id, name, mimeType, modifiedTime, parents"),
            ("addParents", dest_parent_meta.id.as_str()),
            ("removeParents", src_parent_meta.id.as_str()),
        ];
        let body = json!({
            "name": dest.file_name().unwrap().to_str().unwrap(),
        });
        let res = self
            .http_client
            .patch(url)
            .headers(headers)
            .query(&query)
            .json(&body)
            .send()
            .await?;

        let new_meta: GDFile = res.json().await?;
        self.path_to_meta.remove(src);
        self.path_to_meta.insert(dest.to_path_buf(), new_meta);
        Ok(())
    }

    async fn build_tree(&mut self) -> Result<Node> {
        let root_dir_id = self.get_root_dir_id().await?;
        debug!("Root dir id: {}", root_dir_id);

        // TODO consider make self.path_to_meta to arc mut
        let path_to_meta = Arc::new(Mutex::new(HashMap::new()));
        let node = self
            .build_node(&root_dir_id, "".as_ref(), true, path_to_meta.clone())
            .await?;
        self.path_to_meta = path_to_meta.lock().await.clone();

        match node.node_type {
            NodeType::File => Err(Error::ExpectDirectory(self.root_dir.clone())),
            NodeType::Directory => Ok(node),
        }
    }
}

// endregion
