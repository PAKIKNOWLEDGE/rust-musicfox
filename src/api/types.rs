//! Serde types for NetEase Cloud Music API responses.
//!
//! Field names follow the JSON contract of the NetEase open API
//! (aliases handle the shortened keys `ar`/`al`/`dt` used in search results).
//!
//! Many fields are part of the API contract but not yet consumed by the UI;
//! they are kept so the types mirror the wire format as features grow.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Artist {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Album {
    pub id: i64,
    pub name: String,
    #[serde(default, rename = "picUrl")]
    pub pic_url: Option<String>,
    #[serde(default)]
    pub artist: Option<Artist>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
    /// Full track id list; NOT truncated (unlike `tracks`).
    #[serde(default, rename = "trackIds")]
    pub track_ids: Vec<TrackId>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackId {
    pub id: i64,
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
pub struct SongDetailResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub songs: Vec<Song>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountGetResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub account: Option<serde_json::Value>,
    #[serde(default)]
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[serde(default, rename = "userId")]
    pub user_id: i64,
    #[serde(default)]
    pub nickname: String,
    #[serde(default, rename = "avatarUrl")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserPlaylistResp {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub playlist: Vec<Playlist>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personalized_play_count_float() {
        // The API sends playCount as scientific-notation floats; this must
        // not break deserialization (regression for the 3.93E7 bug).
        let json =
            r#"{"code":200,"result":[{"id":1,"name":"x","picUrl":"u","playCount":3.9326708E7}]}"#;
        let resp: PersonalizedResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.code, 200);
        assert_eq!(resp.result.len(), 1);
        assert_eq!(resp.result[0].play_count, Some(3.9326708E7));
    }

    #[test]
    fn song_aliases_short_keys() {
        // Search results use shortened keys ar/al/dt.
        let json = r#"{"id":1,"name":"s","ar":[{"id":9,"name":"a"}],"al":{"id":8,"name":"alb"},"dt":123456,"alia":["x"],"fee":1}"#;
        let s: Song = serde_json::from_str(json).unwrap();
        assert_eq!(s.artist_names(), "a");
        assert_eq!(s.album_name(), "alb");
        assert_eq!(s.duration_secs(), 123);
    }

    #[test]
    fn song_full_keys() {
        // Playlist detail uses full keys artists/album.
        let json =
            r#"{"id":1,"name":"s","artists":[{"id":9,"name":"a"}],"album":{"id":8,"name":"alb"}}"#;
        let s: Song = serde_json::from_str(json).unwrap();
        assert_eq!(s.artist_names(), "a");
        assert_eq!(s.album_name(), "alb");
    }
}
