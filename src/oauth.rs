use std::collections::HashSet;
use std::ops::Add;
use std::path::Path;
use std::process::Stdio;

use anyhow::anyhow;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, TimeDelta, Utc};
use itertools::Itertools;
use log::debug;
use rand;
use rand::RngCore;
use reqwest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use url::Url;

const OAUTH_STATE_LEN: usize = 128;
const OAUTH_PKCE_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenType {
    Bearer, // for now support only this type
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub token_type: TokenType,
    pub scope: HashSet<String>,
    pub id_token: String,
}

impl AuthToken {
    pub fn is_expired(&self) -> bool {
        self.expires_at < Utc::now()
    }

    pub async fn from_response(
        res: reqwest::Response,
        refresh_token: Option<String>,
    ) -> anyhow::Result<Self> {
        let data: serde_json::Value = res.json().await?;
        debug!("Got response: {:#?}", data);

        let expires_in = data.get("expires_in").unwrap().as_i64().unwrap();
        let expires_at = Utc::now().add(TimeDelta::seconds(expires_in));

        let scope: HashSet<String> = data
            .get("access_token")
            .unwrap()
            .as_str()
            .unwrap()
            .split(' ')
            .map(|s| s.to_string())
            .collect();

        let refresh_token = match data.get("refresh_token") {
            Some(refresh_token) => refresh_token.to_string(),
            None => refresh_token.expect("No refresh token found"),
        };

        let token = AuthToken {
            access_token: data.get("access_token").unwrap().to_string(),
            refresh_token,
            expires_at,
            token_type: TokenType::Bearer,
            scope,
            id_token: data.get("id_token").unwrap().as_str().unwrap().to_string(),
        };
        Ok(token)
    }

    pub async fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = String::from_utf8(fs::read(path).await?)?;
        let token: AuthToken = serde_json::from_str(data.as_str())?;
        Ok(token)
    }

    pub async fn to_file(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let data = serde_json::to_string(self)?;
        fs::create_dir_all(path.as_ref().parent().unwrap()).await?;
        fs::write(path, data).await?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct OAuthPublicClient {
    client_id: String,
    client_secret: String,
    auth_url: Url,
    token_url: Url,
    scopes: HashSet<String>,
    state: String,
    pkce: String,
    auth_code: Option<String>,
    tcp_listener: Option<TcpListener>,
    localhost_redirect_port: u16,
}

impl OAuthPublicClient {
    pub fn new(
        client_id: impl ToString,
        client_secret: impl ToString,
        auth_url: Url,
        token_url: Url,
    ) -> anyhow::Result<Self> {
        Ok(OAuthPublicClient {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url,
            token_url,
            scopes: HashSet::new(),
            state: Self::generate_random_str(OAUTH_STATE_LEN),
            pkce: Self::generate_random_str(OAUTH_PKCE_LEN),
            auth_code: None,
            tcp_listener: None,
            localhost_redirect_port: 0,
        })
    }

    fn generate_random_str(len: usize) -> String {
        let mut random_gen = rand::thread_rng();
        let mut buf = vec![0; len];
        random_gen.fill_bytes(&mut buf);
        URL_SAFE_NO_PAD.encode(&buf).to_string()
    }

    pub fn add_scope(mut self, scope: impl AsRef<str>) -> Self {
        self.scopes.insert(scope.as_ref().to_string());
        self
    }

    pub async fn new_auth_token(&mut self) -> anyhow::Result<AuthToken> {
        debug!("Start creating new auth token");
        self.start_redirect_listening().await?;
        self.open_auth_url_in_browser()?;
        self.wait_for_auth_code().await?;
        let token = self.exchange_code().await?;
        debug!("Got token: {:?}", token);
        Ok(token)
    }

    async fn start_redirect_listening(&mut self) -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        self.localhost_redirect_port = listener.local_addr()?.port();
        self.tcp_listener = Some(listener);
        debug!(
            "Start redirect listening on port {}",
            self.localhost_redirect_port
        );
        Ok(())
    }

    fn open_auth_url_in_browser(&self) -> anyhow::Result<()> {
        let full_auth_url = self.full_auth_url().to_string();
        debug!("Opening auth URL: {}", full_auth_url);
        // TODO support window
        Command::new("xdg-open")
            .arg(full_auth_url)
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        Ok(())
    }

    fn full_auth_url(&self) -> Url {
        let mut url = self.auth_url.clone();
        let scopes = self.scopes.iter().join(" ");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("state", &self.state)
            .append_pair("redirect_uri", &self.redirect_uri())
            .append_pair("code_challenge", &self.pkce_hash())
            .append_pair("code_challenge_method", "S256")
            .append_pair("scope", &scopes);
        url
    }

    fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.localhost_redirect_port)
    }

    fn pkce_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.pkce.as_bytes());
        let result = hasher.finalize();
        let res = URL_SAFE_NO_PAD.encode(&result);
        res
    }

    async fn wait_for_auth_code(&mut self) -> anyhow::Result<()> {
        debug!("Waiting for auth code");

        let (mut stream, _) = self.tcp_listener.as_ref().unwrap().accept().await?;

        let buf_reader = io::BufReader::new(&mut stream);
        let first_line = buf_reader.lines().next_line().await?.unwrap();

        match self.parse_auth_code(&first_line) {
            Ok(auth_code) => {
                self.auth_code = Some(auth_code);
                let resp = b"HTTP/1.1 200 OK\r\n\r\nSuccess! Please go back to CLI.\r\n".to_vec();
                self.resp_and_close_http(stream, resp).await?;
                Ok(())
            }
            Err(e) => {
                debug!("Error parsing auth code: {e}");
                self.auth_code = None;
                let resp = format!("HTTP/1.1 500 INTERNAL ERROR\r\n\r\nError! {e}\r\n").into();
                self.resp_and_close_http(stream, resp).await?;
                Err(e)
            }
        }
    }

    fn parse_auth_code(&self, http_req_first_line: &str) -> anyhow::Result<String> {
        debug!("Parsing response HTTP GET: {}", http_req_first_line);
        let mut parts = http_req_first_line.split(" ");
        let full_url = format!("http://127.0.0.1:8080{}", parts.nth(1).unwrap());

        let mut parsed_url = Url::parse(&full_url)?;

        if let Some(err_msg) = Self::get_query_param(&mut parsed_url, "error") {
            return Err(anyhow!(err_msg));
        }

        let granted_scopes =
            Self::get_query_param(&mut parsed_url, "scope").expect("No scopes found");
        if !self.all_scopes_granted(&granted_scopes) {
            return Err(anyhow!(
                "Not all scopes granted. Only granted: {}",
                granted_scopes
            ));
        }

        let code = Self::get_query_param(&mut parsed_url, "code").expect("No code found");
        Ok(code)
    }

    fn get_query_param(parsed_url: &mut Url, param: &str) -> Option<String> {
        parsed_url
            .query_pairs()
            .into_owned()
            .find_map(|(k, v)| if k == param { Some(v) } else { None })
    }

    fn all_scopes_granted(&self, granted_scopes: &str) -> bool {
        let granted_scopes: HashSet<&str> = granted_scopes.split(' ').collect();
        let requested_scopes: HashSet<&str> = self.scopes.iter().map(|x| x.as_str()).collect();
        requested_scopes.is_subset(&granted_scopes)
    }

    async fn resp_and_close_http(
        &self,
        mut stream: TcpStream,
        http_resp: Vec<u8>,
    ) -> io::Result<()> {
        stream.write(&http_resp).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn exchange_code(&self) -> anyhow::Result<AuthToken> {
        debug!("Exchanging code");
        let auth_code = self.auth_code.as_ref().unwrap();
        let params = [
            ("grant_type", &"authorization_code".to_string()),
            ("code", &auth_code),
            ("code_verifier", &self.pkce),
            ("redirect_uri", &self.redirect_uri()),
        ];
        self.req_auth_server(&params).await
    }

    pub async fn refresh_token(&mut self, token: &mut AuthToken) -> anyhow::Result<AuthToken> {
        debug!("Refreshing token");
        let params = [
            ("grant_type", &"refresh_token".to_string()),
            ("refresh_token", &token.refresh_token),
        ];
        self.req_auth_server(&params).await
    }

    async fn req_auth_server(&self, extra_params: &[(&str, &String)]) -> anyhow::Result<AuthToken> {
        let mut params = vec![
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];
        params.extend_from_slice(extra_params);
        debug!("Making req to auth server with params: {:?}", params);

        let refresh_token = extra_params.iter().find_map(|(k, v)| {
            if k.eq(&"refresh_token") {
                Some((**v).clone())
            } else {
                None
            }
        });

        let client = reqwest::Client::new();
        let res = client
            .post(self.token_url.clone())
            .form(&params)
            .send()
            .await?;

        debug!("Get status: {}", res.status());
        if res.status() != reqwest::StatusCode::OK {
            return Err(anyhow!(
                "Auth server returned unexpected code: {}",
                res.status()
            ));
        }

        Ok(AuthToken::from_response(res, refresh_token).await?)
    }
}
