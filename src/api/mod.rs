//! NetEase Cloud Music API client.
//!
//! Content endpoints (search, playlists, song URLs, lyrics) use the legacy
//! plain-GET API which is reliable and needs no request encryption. Login
//! supports two channels:
//!
//! - QR login through the weapi endpoints (best effort; some networks block
//!   these with anti-bot 302/empty responses),
//! - cookie paste: the user provides the `MUSIC_U` session cookie from their
//!   browser, which works on any network.
//!
//! Cookies are persisted to a JSON file under the user config dir so a
//! login survives restarts.

pub mod types;
pub mod weapi;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use reqwest::header::{COOKIE, REFERER};
use reqwest::Client;
use serde_json::{json, Value};

use types::*;

pub const BASE: &str = "https://music.163.com";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Default config/cookie directory: `<config>/rust-musicfox`.
pub fn data_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rust-musicfox")
}

pub struct NeteaseClient {
    http: Client,
    cookies: HashMap<String, String>,
    cookie_path: Option<PathBuf>,
}

impl NeteaseClient {
    pub fn new(cookie_path: Option<PathBuf>) -> Result<Self> {
        let cookies = match &cookie_path {
            Some(path) if path.exists() => {
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("read cookies from {}", path.display()))?;
                serde_json::from_str(&raw).unwrap_or_default()
            }
            _ => HashMap::new(),
        };
        let http = Client::builder()
            .user_agent(UA)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build http client")?;
        Ok(NeteaseClient {
            http,
            cookies,
            cookie_path,
        })
    }

    /// True when a MUSIC_U session cookie is present.
    pub fn is_logged_in(&self) -> bool {
        self.cookies.contains_key("MUSIC_U")
    }

    /// Clone of the underlying HTTP client (e.g. for audio downloads).
    pub fn http(&self) -> Client {
        self.http.clone()
    }

    /// Set session cookies from a raw cookie string ("k=v; k2=v2; ...").
    /// Persists immediately when a cookie path is configured.
    pub fn set_cookie_str(&mut self, cookie_str: &str) -> Result<()> {
        self.merge_cookie_str(cookie_str);
        if !self.is_logged_in() {
            return Err(anyhow!(
                "cookie 中未找到 MUSIC_U（请从浏览器复制完整 cookie）"
            ));
        }
        Ok(())
    }

    fn cookie_header(&self) -> String {
        let mut parts = vec!["os=pc".to_string(), "appver=2.9.7".to_string()];
        for (k, v) in &self.cookies {
            parts.push(format!("{}={}", k, v));
        }
        parts.join("; ")
    }

    fn save_cookies(&self) {
        if let Some(path) = &self.cookie_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&self.cookies) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    fn merge_cookie_str(&mut self, cookie_str: &str) {
        for part in cookie_str.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                if !k.is_empty() && !is_cookie_attr(k) {
                    self.cookies.insert(k.to_string(), v.to_string());
                }
            }
        }
        self.save_cookies();
    }

    fn check_code(&self, body: &Value) -> Result<()> {
        if let Some(code) = body.get("code").and_then(|c| c.as_i64()) {
            if code != 200 {
                return Err(anyhow!("{}", ApiError::from_code(code)));
            }
        }
        Ok(())
    }

    /// Legacy plain-GET endpoint with browser-ish headers.
    async fn api_get(&self, path: &str) -> Result<Value> {
        let resp = self
            .http
            .get(format!("{}{}", BASE, path))
            .header(REFERER, BASE)
            .header(COOKIE, self.cookie_header())
            .send()
            .await
            .with_context(|| format!("request failed: {}", path))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .with_context(|| format!("read response from {} (status {})", path, status))?;
        if text.is_empty() {
            return Err(anyhow!(
                "empty response from {} (status {}), 可能被风控拦截",
                path,
                status
            ));
        }
        let body: Value = serde_json::from_str(&text)
            .with_context(|| format!("parse response from {} (status {})", path, status))?;
        self.check_code(&body)?;
        Ok(body)
    }

    /// weapi POST (used for QR login; some networks block these).
    async fn weapi_post(&self, path: &str, data: Value) -> Result<Value> {
        let (params, enc_sec_key) = weapi::weapi_params(&data);
        let resp = self
            .http
            .post(format!("{}{}", BASE, path))
            .header(REFERER, BASE)
            .header(COOKIE, self.cookie_header())
            .form(&[
                ("params", params.as_str()),
                ("encSecKey", enc_sec_key.as_str()),
            ])
            .send()
            .await
            .context("weapi request failed")?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .with_context(|| format!("read response from {} (status {})", path, status))?;
        if text.is_empty() {
            return Err(anyhow!(
                "empty response from {} (status {}), 可能被风控拦截",
                path,
                status
            ));
        }
        let body: Value = serde_json::from_str(&text)
            .with_context(|| format!("parse response from {} (status {})", path, status))?;
        self.check_code(&body)?;
        Ok(body)
    }

    fn timestamp() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    // ---- login ----

    pub async fn qr_key(&self) -> Result<String> {
        let body = self
            .weapi_post(
                "/weapi/login/qr/key",
                json!({"timestamp": Self::timestamp()}),
            )
            .await?;
        let resp: QrKeyResp = serde_json::from_value(body).context("parse qr key response")?;
        if resp.unikey.is_empty() {
            return Err(anyhow!("empty unikey"));
        }
        Ok(resp.unikey)
    }

    pub async fn qr_create(&self, key: &str) -> Result<String> {
        let body = self
            .weapi_post(
                "/weapi/login/qr/create",
                json!({"key": key, "qrcodeWidth": 400, "timestamp": Self::timestamp()}),
            )
            .await?;
        let resp: QrCreateResp =
            serde_json::from_value(body).context("parse qr create response")?;
        if resp.qrurl.is_empty() {
            return Err(anyhow!("empty qr url"));
        }
        Ok(resp.qrurl)
    }

    /// Poll QR status. On success (803) the session cookies are captured.
    pub async fn qr_check(&mut self, key: &str) -> Result<QrCheckResp> {
        let body = self
            .weapi_post(
                "/weapi/login/qr/check",
                json!({"key": key, "timestamp": Self::timestamp()}),
            )
            .await?;
        let resp: QrCheckResp = serde_json::from_value(body).context("parse qr check response")?;
        if resp.code == 803 {
            self.merge_cookie_str(&resp.cookie);
        }
        Ok(resp)
    }

    // ---- content (legacy plain API) ----

    pub async fn search(&self, keywords: &str, limit: u32) -> Result<Vec<Song>> {
        let body = self
            .api_get(&format!(
                "/api/search/get/web?csrf_token=&s={}&type=1&offset=0&limit={}",
                urlencoding::encode(keywords),
                limit
            ))
            .await?;
        let resp: SearchResp = serde_json::from_value(body).context("parse search response")?;
        Ok(resp.result.map(|r| r.songs).unwrap_or_default())
    }

    pub async fn personalized_playlists(&self, limit: u32) -> Result<Vec<PersonalizedItem>> {
        let body = self
            .api_get(&format!("/api/personalized/playlist?limit={}", limit))
            .await?;
        let resp: PersonalizedResp =
            serde_json::from_value(body).context("parse personalized response")?;
        Ok(resp.result)
    }

    pub async fn toplists(&self) -> Result<Vec<ToplistItem>> {
        let body = self.api_get("/api/toplist").await?;
        let resp: ToplistResp = serde_json::from_value(body).context("parse toplist response")?;
        Ok(resp.list)
    }

    pub async fn playlist_detail(&self, id: i64) -> Result<Playlist> {
        let body = self
            .api_get(&format!("/api/v3/playlist/detail?id={}", id))
            .await?;
        let resp: PlaylistDetailResp =
            serde_json::from_value(body).context("parse playlist detail response")?;
        resp.playlist.ok_or_else(|| anyhow!("playlist not found"))
    }

    pub async fn recommend_songs(&self) -> Result<Vec<Song>> {
        let body = self
            .api_get("/api/v3/discovery/recommend/songs?csrf_token=")
            .await?;
        let resp: RecommendSongsResp =
            serde_json::from_value(body).context("parse recommend response")?;
        Ok(resp.data.map(|d| d.daily_songs).unwrap_or_default())
    }

    /// Playable URL for a song; `br` is the bitrate (128000 / 320000 / ...).
    pub async fn song_url(&self, id: i64, br: u32) -> Result<String> {
        let body = self
            .api_get(&format!(
                "/api/song/enhance/player/url?ids=%5B{}%5D&br={}",
                id, br
            ))
            .await?;
        let resp: SongUrlResp = serde_json::from_value(body).context("parse song url response")?;
        resp.data
            .first()
            .and_then(|d| d.url.clone())
            .ok_or_else(|| anyhow!("no playable url (VIP or region restricted)"))
    }

    pub async fn lyric(&self, id: i64) -> Result<LyricResp> {
        let body = self
            .api_get(&format!("/api/song/lyric?id={}&lv=-1&kv=-1&tv=-1", id))
            .await?;
        let resp: LyricResp = serde_json::from_value(body).context("parse lyric response")?;
        Ok(resp)
    }

    pub async fn logout(&mut self) -> Result<()> {
        let _ = self.api_get("/api/logout?csrf_token=").await;
        self.cookies.remove("MUSIC_U");
        self.save_cookies();
        Ok(())
    }
}

fn is_cookie_attr(k: &str) -> bool {
    k.eq_ignore_ascii_case("Path")
        || k.eq_ignore_ascii_case("Max-Age")
        || k.eq_ignore_ascii_case("Expires")
        || k.eq_ignore_ascii_case("Domain")
        || k.eq_ignore_ascii_case("HttpOnly")
        || k.eq_ignore_ascii_case("Secure")
        || k.eq_ignore_ascii_case("SameSite")
}
