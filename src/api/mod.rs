//! NetEase Cloud Music API client.
//!
//! All endpoints use the PLAINTEXT legacy API under `/api/`, which is
//! reliable on networks where the `/weapi/` paths are blocked by anti-bot
//! (empty 200 responses). QR login works the same way: the plaintext
//! `/api/login/qrcode/*` endpoints replace go-musicfox's weapi variants, so
//! no weapi encryption or TLS fingerprint impersonation is needed.
//!
//! Login supports two channels:
//!
//! - QR scan (plaintext endpoints, works on most networks),
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
use serde_json::Value;

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
    /// Dedicated client for large audio downloads: the API client's 30s
    /// overall timeout would kill slow downloads of multi-MB files.
    download_http: Client,
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
        let download_http = Client::builder()
            .user_agent(UA)
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .context("build download client")?;
        Ok(NeteaseClient {
            http,
            download_http,
            cookies,
            cookie_path,
        })
    }

    /// True when a MUSIC_U session cookie is present.
    pub fn is_logged_in(&self) -> bool {
        self.cookies.contains_key("MUSIC_U")
    }

    /// Clone of the API HTTP client (short requests, 30s cap).
    pub fn http(&self) -> Client {
        self.http.clone()
    }

    /// Clone of the download HTTP client (large bodies, 5min cap).
    pub fn download_http(&self) -> Client {
        self.download_http.clone()
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

    // ---- login ----
    //
    // QR login uses the PLAINTEXT endpoints under /api/ (unlike go-musicfox's
    // weapi variants). The /weapi/ paths are blocked by anti-bot on many
    // networks (empty 200 responses), while /api/ works everywhere — no weapi
    // encryption and no TLS fingerprint impersonation needed.

    /// Request a QR login unikey.
    pub async fn qr_key(&self) -> Result<String> {
        let body = self
            .api_get("/api/login/qrcode/unikey?type=1&noCheckToken=true")
            .await?;
        let resp: QrKeyResp = serde_json::from_value(body).context("parse qr key response")?;
        if resp.unikey.is_empty() {
            return Err(anyhow!("empty unikey"));
        }
        Ok(resp.unikey)
    }

    /// Build the scan URL for a unikey.
    pub fn qr_url(&self, key: &str) -> String {
        format!("http://music.163.com/login?codekey={}", key)
    }

    /// Poll QR status. Intermediate states (801 waiting / 802 scanned) are
    /// normal and must NOT be treated as errors, so this bypasses the
    /// code==200 check of `api_get`. On success (803) the session cookies
    /// are captured from the body AND from Set-Cookie response headers.
    pub async fn qr_check(&mut self, key: &str) -> Result<QrCheckResp> {
        let url = format!(
            "{}/api/login/qrcode/client/login?type=1&noCheckToken=true&key={}",
            BASE,
            urlencoding::encode(key)
        );
        let resp = self
            .http
            .get(&url)
            .header(REFERER, BASE)
            .header(COOKIE, self.cookie_header())
            .send()
            .await
            .with_context(|| format!("request failed: {}", url))?;
        let status = resp.status();
        // Collect Set-Cookie headers before consuming the body.
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        let text = resp
            .text()
            .await
            .with_context(|| format!("read qr check response (status {})", status))?;
        if text.is_empty() {
            return Err(anyhow!(
                "empty qr check response (status {}), 可能被风控拦截",
                status
            ));
        }
        let body: Value = serde_json::from_str(&text).context("parse qr check response")?;
        let mut parsed: QrCheckResp =
            serde_json::from_value(body).context("parse qr check response")?;
        if parsed.code == 803 {
            self.merge_cookie_str(&parsed.cookie);
            // Some endpoints deliver the session via Set-Cookie headers only.
            for cookie in &set_cookies {
                self.merge_cookie_str(cookie);
            }
        }
        // Normalize: 801/802 are pending states, not errors.
        parsed.message = match parsed.code {
            801 => "等待扫码".into(),
            802 => "已扫码，请在手机上确认".into(),
            _ => parsed.message,
        };
        Ok(parsed)
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

    /// High-quality (精选) playlists for a category; `cat` "全部" or a
    /// category name such as "华语".
    pub async fn highquality_playlists(&self, cat: &str, limit: u32) -> Result<Vec<Playlist>> {
        let body = self
            .api_get(&format!(
                "/api/playlist/highquality/list?cat={}&limit={}",
                urlencoding::encode(cat),
                limit
            ))
            .await?;
        let resp: HighQualityResp =
            serde_json::from_value(body).context("parse high quality playlist response")?;
        Ok(resp.playlists)
    }

    /// Current account profile (login required); None when logged out.
    pub async fn account_profile(&self) -> Result<Option<Profile>> {
        let body = self.api_get("/api/nuser/account/get").await?;
        let resp: AccountGetResp =
            serde_json::from_value(body).context("parse account response")?;
        Ok(resp.profile)
    }

    /// Playlists created/collected by a user.
    pub async fn user_playlists(&self, uid: i64, limit: u32) -> Result<Vec<Playlist>> {
        let body = self
            .api_get(&format!(
                "/api/user/playlist?uid={}&limit={}&offset=0",
                uid, limit
            ))
            .await?;
        let resp: UserPlaylistResp =
            serde_json::from_value(body).context("parse user playlist response")?;
        Ok(resp.playlist)
    }

    pub async fn playlist_detail(&self, id: i64) -> Result<Playlist> {
        // n/s params are required: without them the API only returns the
        // first 10 tracks even for playlists with hundreds of songs.
        let body = self
            .api_get(&format!("/api/v3/playlist/detail?id={}&n=100000&s=8", id))
            .await?;
        let resp: PlaylistDetailResp =
            serde_json::from_value(body).context("parse playlist detail response")?;
        resp.playlist.ok_or_else(|| anyhow!("playlist not found"))
    }

    /// Song details for up to 500 ids via the plaintext endpoint.
    pub async fn song_detail(&self, ids: &[i64]) -> Result<Vec<Song>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids_str = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let body = self
            .api_get(&format!("/api/song/detail?ids=%5B{}%5D", ids_str))
            .await?;
        let resp: SongDetailResp =
            serde_json::from_value(body).context("parse song detail response")?;
        Ok(resp.songs)
    }

    /// ALL tracks of a playlist. `playlist/detail` truncates `tracks` (10 in
    /// practice) but its `trackIds` is complete; we fetch song details in
    /// concurrent batches of 500 (mirroring go-musicfox's
    /// PlaylistTrackAllService) and reorder by the track id list.
    pub async fn playlist_all_tracks(&self, id: i64) -> Result<Vec<Song>> {
        let playlist = self.playlist_detail(id).await?;
        let ids: Vec<i64> = playlist.track_ids.iter().map(|t| t.id).collect();
        if ids.is_empty() {
            return Ok(playlist.tracks.unwrap_or_default());
        }
        let mut by_id: std::collections::HashMap<i64, Song> = std::collections::HashMap::new();
        let mut set = tokio::task::JoinSet::new();
        let http = self.http.clone();
        // The plaintext song/detail endpoint caps at ~201 songs per request,
        // so batch at 200 (go-musicfox uses 500, but its weapi variant has a
        // higher cap; 200 keeps us safely under the plaintext limit).
        for chunk in ids.chunks(200) {
            let http = http.clone();
            let ids_str = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            set.spawn(async move {
                let url = format!("{}/api/song/detail?ids=%5B{}%5D", BASE, ids_str);
                let resp = http.get(&url).header(REFERER, BASE).send().await?;
                let status = resp.status();
                let text = resp.text().await?;
                if !status.is_success() || text.is_empty() {
                    return Err(anyhow!("song detail 请求失败 (status {})", status));
                }
                let body: Value = serde_json::from_str(&text)
                    .with_context(|| format!("parse song detail response (status {})", status))?;
                let parsed: SongDetailResp =
                    serde_json::from_value(body).context("parse song detail response")?;
                Ok::<_, anyhow::Error>(parsed.songs)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok(Ok(songs)) = joined {
                for s in songs {
                    by_id.insert(s.id, s);
                }
            }
        }
        // Reorder to match the playlist's track id order.
        Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
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
