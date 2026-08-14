//! Serde types for NetEase Cloud Music API responses.
//!
//! Field names follow the JSON contract of the NetEase open API
//! (aliases handle the shortened keys `ar`/`al`/`dt` used in search results).
//!
//! Many fields are part of the API contract but not yet consumed by the UI;
//! they are kept so the types mirror the wire format as features grow.
#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Artist {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Album {
    pub id: i64,
    pub name: String,
    #[serde(default, rename = "picUrl")]
    pub pic_url: Option<String>,
    #[serde(default)]
    pub artist: Option<Artist>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Song {
    pub id: i64,
    pub name: String,
    #[serde(default, alias = "ar")]
    pub artists: Vec<Artist>,
    #[serde(default, alias = "al")]
    pub album: Option<Album>,
    #[serde(default)]
    pub alia: Vec<String>,
    #[serde(default, alias = "dt")]
    pub duration: Option<i64>,
    #[serde(default)]
    pub fee: Option<i64>,
}

impl Song {
    pub fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    }

    pub fn album_name(&self) -> String {
        self.album
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default()
    }

    pub fn duration_secs(&self) -> u64 {
        (self.duration.unwrap_or(0) as u64) / 1000
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    #[serde(default, rename = "coverImgUrl")]
    pub cover_img_url: Option<String>,
    #[serde(default, rename = "trackCount")]
    pub track_count: Option<i64>,
    #[serde(default, rename = "playCount")]
    pub play_count: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub creator: Option<PlaylistCreator>,
    #[serde(default)]
    pub tracks: Option<Vec<Song>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlaylistCreator {
    #[serde(default)]
    pub nickname: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrKeyResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub unikey: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCreateResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub qrurl: String,
}

/// QR check: 800=expired, 801=waiting, 802=scanned-pending, 803=success.
#[derive(Debug, Clone, Deserialize)]
pub struct QrCheckResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub cookie: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SongUrlResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub data: Vec<SongUrlItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SongUrlItem {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub br: Option<i64>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LyricResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub lrc: Option<LyricBlock>,
    #[serde(default)]
    pub tlyric: Option<LyricBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LyricBlock {
    #[serde(default)]
    pub lyric: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub result: Option<SearchResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub songs: Vec<Song>,
    #[serde(default, rename = "songCount")]
    pub song_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalizedResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub result: Vec<PersonalizedItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalizedItem {
    pub id: i64,
    pub name: String,
    #[serde(default, rename = "picUrl")]
    pub pic_url: Option<String>,
    /// Play count; the API sends scientific-notation floats (e.g. 3.93E7).
    #[serde(default, rename = "playCount")]
    pub play_count: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecommendSongsResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub data: Option<RecommendData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecommendData {
    #[serde(default, rename = "dailySongs")]
    pub daily_songs: Vec<Song>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaylistDetailResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub playlist: Option<Playlist>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToplistResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub list: Vec<ToplistItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HighQualityResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToplistItem {
    pub id: i64,
    pub name: String,
    #[serde(default, rename = "coverImgUrl")]
    pub cover_img_url: Option<String>,
    #[serde(default, rename = "updateFrequency")]
    pub update_frequency: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Basic API error with the NetEase status code.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("netease api error: code={code} message={message}")]
    Code { code: i64, message: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ApiError {
    pub fn from_code(code: i64) -> Self {
        ApiError::Code {
            code,
            message: match code {
                301 => "需要登录或登录已过期".into(),
                462 => "请求过于频繁，被风控拦截".into(),
                200 => "操作成功".into(),
                _ => format!("未知错误 {}", code),
            },
        }
    }
}
