//! TUI application: state, event loop, rendering.
//!
//! Architecture: a single async main task owns the `Player` and renders.
//! Crossterm key events arrive through a channel from a reader thread.
//! Network work (playlist fetch, search, QR polling, song download) runs in
//! spawned tokio tasks that report back through an internal message channel,
//! so the audio device is only ever touched by the main task.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use tokio::sync::{mpsc, Mutex};

use crate::api::types::{PersonalizedItem, Playlist, Song, ToplistItem};
use crate::api::NeteaseClient;
use crate::lyric::{self, LyricLine};
use crate::player::{PlayState, Player};

pub const SONG_BR: u32 = 128000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoginMode {
    Qr,
    Cookie,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlayMode {
    ListLoop,
    SingleLoop,
    Shuffle,
    Sequence,
}

impl PlayMode {
    fn label(&self) -> &'static str {
        match self {
            PlayMode::ListLoop => "列表循环",
            PlayMode::SingleLoop => "单曲循环",
            PlayMode::Shuffle => "随机播放",
            PlayMode::Sequence => "顺序播放",
        }
    }

    fn next(self) -> Self {
        match self {
            PlayMode::ListLoop => PlayMode::SingleLoop,
            PlayMode::SingleLoop => PlayMode::Shuffle,
            PlayMode::Shuffle => PlayMode::Sequence,
            PlayMode::Sequence => PlayMode::ListLoop,
        }
    }
}

enum View {
    Main,
    PlaylistList,
    Square,
    TopList,
    PlaylistDetail { id: i64, name: String },
    Search,
    Player,
    Queue,
    Help,
    Login,
}

enum MainItem {
    Daily,
    Personalized,
    Square,
    Toplist,
    Search,
    Login,
    Quit,
}

impl MainItem {
    fn label(&self, logged_in: bool) -> &'static str {
        match self {
            MainItem::Daily => "每日推荐 (需要登录)",
            MainItem::Personalized => "推荐歌单",
            MainItem::Square => "歌单广场",
            MainItem::Toplist => "榜单",
            MainItem::Search => "搜索",
            MainItem::Login => {
                if logged_in {
                    "退出登录"
                } else {
                    "登录 (扫码/Cookie)"
                }
            }
            MainItem::Quit => "退出",
        }
    }
}

/// Popular categories for the playlist square (the category list API is not
/// exposed through the legacy plain endpoints, so we hardcode popular ones).
const SQUARE_CATS: [&str; 10] = [
    "全部",
    "华语",
    "欧美",
    "流行",
    "摇滚",
    "民谣",
    "电子",
    "轻音乐",
    "ACG",
    "怀旧",
];

/// Messages from spawned network tasks to the main loop.
enum Msg {
    PlaylistReady {
        id: i64,
        name: String,
        songs: Vec<Song>,
    },
    PlaylistListReady {
        items: Vec<PersonalizedItem>,
    },
    ToplistReady {
        items: Vec<ToplistItem>,
    },
    SquareReady {
        items: Vec<Playlist>,
    },
    SearchReady {
        songs: Vec<Song>,
    },
    PlaybackReady {
        song: Song,
        bytes: Vec<u8>,
    },
    LyricsReady {
        lines: Vec<LyricLine>,
        trans: Vec<LyricLine>,
    },
    DownloadDone(String),
    OpError(String),
    QrKeyReady {
        key: String,
        qr: String,
    },
    QrStatus {
        code: i64,
        message: String,
    },
    CookieLoginOk,
    LoggedOut,
}

pub struct App {
    client: Arc<Mutex<NeteaseClient>>,
    player: Player,
    tx: mpsc::UnboundedSender<Msg>,
    rx: Option<mpsc::UnboundedReceiver<Msg>>,
    view: View,
    back_stack: Vec<View>,

    cfg: crate::config::Config,
    logged_in: bool,

    // main menu
    main_index: usize,

    // playlist list ("推荐歌单")
    playlists: Vec<PersonalizedItem>,
    playlists_index: usize,
    playlists_loading: bool,

    // playlist square ("歌单广场")
    square_cat_index: usize,
    square_playlists: Vec<Playlist>,
    square_index: usize,
    square_loading: bool,

    // top lists ("榜单")
    toplists: Vec<ToplistItem>,
    toplists_index: usize,
    toplists_loading: bool,

    // playlist detail / daily recommend
    playlist_songs: Vec<Song>,
    playlist_index: usize,
    loading: bool,

    // search
    search_query: String,
    search_input_mode: bool,
    search_results: Vec<Song>,
    search_index: usize,
    search_loading: bool,

    // player
    queue: Vec<Song>,
    queue_index: usize,
    queue_cursor: usize,
    current: Option<Song>,
    lyrics: Vec<LyricLine>,
    tlyric: Vec<LyricLine>,
    lyric_index: usize,
    current_song_id: i64,
    play_mode: PlayMode,
    /// True while the next song's audio is still downloading; prevents the
    /// auto-advance loop from skipping through the queue when the sink is
    /// momentarily empty.
    loading_next: bool,

    // login
    qr_unikey: Option<String>,
    qr_string: Option<String>,
    qr_message: String,
    login_mode: LoginMode,

    status_msg: String,
    quitting: bool,
}

impl App {
    pub fn new(client: NeteaseClient, mut player: Player) -> Result<Self> {
        let logged_in = client.is_logged_in();
        let cfg = crate::config::Config::load().unwrap_or_default();
        player.set_volume(cfg.volume);
        let play_mode = match cfg.play_mode.as_str() {
            "single" => PlayMode::SingleLoop,
            "shuffle" => PlayMode::Shuffle,
            "sequence" => PlayMode::Sequence,
            _ => PlayMode::ListLoop,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        Ok(App {
            client: Arc::new(Mutex::new(client)),
            player,
            tx,
            rx: Some(rx),
            view: View::Main,
            back_stack: Vec::new(),
            cfg,
            logged_in,
            main_index: 0,
            playlists: Vec::new(),
            playlists_index: 0,
            playlists_loading: false,
            square_cat_index: 0,
            square_playlists: Vec::new(),
            square_index: 0,
            square_loading: false,
            toplists: Vec::new(),
            toplists_index: 0,
            toplists_loading: false,
            playlist_songs: Vec::new(),
            playlist_index: 0,
            loading: false,
            search_query: String::new(),
            search_input_mode: false,
            search_results: Vec::new(),
            search_index: 0,
            search_loading: false,
            queue: Vec::new(),
            queue_index: 0,
            queue_cursor: 0,
            current: None,
            lyrics: Vec::new(),
            tlyric: Vec::new(),
            lyric_index: 0,
            current_song_id: 0,
            play_mode,
            loading_next: false,
            qr_unikey: None,
            qr_string: None,
            qr_message: String::new(),
            login_mode: LoginMode::Qr,
            status_msg: String::new(),
            quitting: false,
        })
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = msg.into();
    }

    fn push_view(&mut self, view: View) {
        let prev = std::mem::replace(&mut self.view, view);
        self.back_stack.push(prev);
    }

    fn pop_view(&mut self) {
        if let Some(prev) = self.back_stack.pop() {
            self.view = prev;
        }
    }

    fn go_main(&mut self) {
        self.view = View::Main;
        self.back_stack.clear();
    }

    // ---- navigation / async actions ----

    fn open_playlist(&mut self, id: i64, name: String) {
        self.playlist_songs.clear();
        self.playlist_index = 0;
        self.loading = true;
        self.push_view(View::PlaylistDetail {
            id,
            name: name.clone(),
        });
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                let playlist = client.playlist_detail(id).await?;
                let songs = playlist.tracks.unwrap_or_default();
                Ok::<_, anyhow::Error>((name, songs))
            }
            .await;
            match result {
                Ok((name, songs)) => {
                    let _ = tx.send(Msg::PlaylistReady { id, name, songs });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("加载歌单失败: {}", e)));
                }
            }
        });
    }

    fn load_daily(&mut self) {
        self.playlist_songs.clear();
        self.playlist_index = 0;
        self.loading = true;
        self.push_view(View::PlaylistDetail {
            id: 0,
            name: "每日推荐".into(),
        });
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                client.recommend_songs().await
            }
            .await;
            match result {
                Ok(songs) => {
                    let _ = tx.send(Msg::PlaylistReady {
                        id: 0,
                        name: "每日推荐".into(),
                        songs,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("每日推荐失败: {}", e)));
                }
            }
        });
    }

    fn load_playlists(&mut self) {
        self.playlists.clear();
        self.playlists_index = 0;
        self.playlists_loading = true;
        self.push_view(View::PlaylistList);
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                client.personalized_playlists(30).await
            }
            .await;
            match result {
                Ok(items) => {
                    let _ = tx.send(Msg::PlaylistListReady { items });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("推荐歌单失败: {}", e)));
                }
            }
        });
    }

    fn load_square(&mut self) {
        self.square_playlists.clear();
        self.square_index = 0;
        self.square_loading = true;
        self.push_view(View::Square);
        self.fetch_square();
    }

    fn fetch_square(&mut self) {
        let cat = SQUARE_CATS[self.square_cat_index].to_string();
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                client.highquality_playlists(&cat, 30).await
            }
            .await;
            match result {
                Ok(items) => {
                    let _ = tx.send(Msg::SquareReady { items });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("歌单广场加载失败: {}", e)));
                }
            }
        });
    }

    fn load_toplists(&mut self) {
        self.toplists.clear();
        self.toplists_index = 0;
        self.toplists_loading = true;
        self.push_view(View::TopList);
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                client.toplists().await
            }
            .await;
            match result {
                Ok(items) => {
                    let _ = tx.send(Msg::ToplistReady { items });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("榜单加载失败: {}", e)));
                }
            }
        });
    }

    fn start_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_loading = true;
        self.search_results.clear();
        self.search_index = 0;
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                client.search(&query, 30).await
            }
            .await;
            match result {
                Ok(songs) => {
                    let _ = tx.send(Msg::SearchReady { songs });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("搜索失败: {}", e)));
                }
            }
        });
    }

    fn enter_login(&mut self) {
        if self.logged_in {
            let client = self.client.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let mut client = client.lock().await;
                let _ = client.logout().await;
                let _ = tx.send(Msg::LoggedOut);
            });
            return;
        }
        self.qr_unikey = None;
        self.qr_string = None;
        self.qr_message = "正在获取二维码...".into();
        self.login_mode = LoginMode::Qr;
        self.push_view(View::Login);
        self.refresh_qr();
    }

    /// Cookie file location: `<config>/rust-musicfox/cookie.txt`
    fn cookie_file_path() -> std::path::PathBuf {
        crate::api::data_dir().join("cookie.txt")
    }

    /// Read a MUSIC_U cookie string from the cookie file and log in.
    fn login_from_cookie_file(&mut self) {
        let path = Self::cookie_file_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s.trim().to_string(),
            Err(e) => {
                self.set_status(format!("读取 cookie 文件失败: {}", e));
                return;
            }
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut client = client.lock().await;
            match client.set_cookie_str(&content) {
                Ok(()) => {
                    let _ = tx.send(Msg::CookieLoginOk);
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("cookie 登录失败: {}", e)));
                }
            }
        });
    }

    fn refresh_qr(&mut self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                let key = client.qr_key().await?;
                let url = client.qr_url(&key);
                Ok::<_, anyhow::Error>((key, url))
            }
            .await;
            match result {
                Ok((key, url)) => {
                    let qr = qrcode::QrCode::new(url)
                        .ok()
                        .map(|c| {
                            c.render::<qrcode::render::unicode::Dense1x2>()
                                .quiet_zone(false)
                                .build()
                        })
                        .unwrap_or_default();
                    let _ = tx.send(Msg::QrKeyReady { key, qr });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!(
                        "获取二维码失败（当前网络 weapi 可能被风控）: {}。请按 c 用 Cookie 登录，或更换网络后按 r 重试",
                        e
                    )));
                }
            }
        });
    }

    fn poll_qr(&mut self) {
        let Some(key) = self.qr_unikey.clone() else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let mut client = client.lock().await;
                client.qr_check(&key).await
            }
            .await;
            match result {
                Ok(resp) => {
                    let _ = tx.send(Msg::QrStatus {
                        code: resp.code,
                        message: match resp.code {
                            800 => "二维码已过期，正在刷新...".into(),
                            801 => "等待扫码...".into(),
                            802 => "已扫码，请在手机上确认".into(),
                            803 => "登录成功!".into(),
                            _ => resp.message,
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("扫码检测失败: {}", e)));
                }
            }
        });
    }

    fn play_at(&mut self, list: Vec<Song>, index: usize) {
        if list.is_empty() {
            return;
        }
        self.queue = list;
        self.queue_index = index.min(self.queue.len() - 1);
        self.play_current();
    }

    fn play_current(&mut self) {
        let Some(song) = self.queue.get(self.queue_index).cloned() else {
            return;
        };
        self.current = Some(song.clone());
        self.current_song_id = song.id;
        self.loading_next = true;
        self.lyrics.clear();
        self.tlyric.clear();
        self.lyric_index = 0;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let br = self.cfg.br;
        tokio::spawn(async move {
            // lyrics (fire-and-forget)
            {
                let lyric_client = client.clone();
                let lyric_tx = tx.clone();
                let sid = song.id;
                tokio::spawn(async move {
                    let result = async {
                        let client = lyric_client.lock().await;
                        let resp = client.lyric(sid).await?;
                        let lrc = resp.lrc.map(|b| b.lyric).unwrap_or_default();
                        let trc = resp.tlyric.map(|b| b.lyric).unwrap_or_default();
                        Ok::<_, anyhow::Error>((lyric::parse_lrc(&lrc), lyric::parse_lrc(&trc)))
                    }
                    .await;
                    if let Ok((lines, trans)) = result {
                        let _ = lyric_tx.send(Msg::LyricsReady { lines, trans });
                    }
                });
            }
            // download audio
            let result = async {
                let client = client.lock().await;
                let http = client.http();
                let url = client.song_url(song.id, br).await?;
                let resp = http.get(&url).send().await?;
                let status = resp.status();
                let bytes = resp.bytes().await?;
                if !status.is_success() || bytes.is_empty() {
                    return Err(anyhow!("下载失败: HTTP {}", status));
                }
                Ok::<_, anyhow::Error>(bytes.to_vec())
            }
            .await;
            match result {
                Ok(bytes) => {
                    let _ = tx.send(Msg::PlaybackReady { song, bytes });
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("播放失败: {}", e)));
                }
            }
        });
    }

    fn next_song(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let len = self.queue.len();
        match self.play_mode {
            PlayMode::SingleLoop => {
                self.play_current();
            }
            PlayMode::Shuffle => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let next = if len == 1 {
                    0
                } else {
                    loop {
                        let i = rng.gen_range(0..len);
                        if i != self.queue_index {
                            break i;
                        }
                    }
                };
                self.queue_index = next;
                self.play_current();
            }
            _ => {
                if self.queue_index + 1 < len {
                    self.queue_index += 1;
                    self.play_current();
                } else if self.play_mode == PlayMode::ListLoop {
                    self.queue_index = 0;
                    self.play_current();
                } else {
                    self.player.stop();
                }
            }
        }
    }

    fn prev_song(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        if self.queue_index > 0 {
            self.queue_index -= 1;
            self.play_current();
        } else {
            // at first song: restart it, or wrap to last in list-loop mode
            if self.play_mode == PlayMode::ListLoop {
                self.queue_index = self.queue.len() - 1;
                self.play_current();
            } else {
                self.player.seek(Duration::ZERO);
            }
        }
    }

    fn toggle_play_mode(&mut self) {
        self.play_mode = self.play_mode.next();
        self.cfg.play_mode = match self.play_mode {
            PlayMode::ListLoop => "list",
            PlayMode::SingleLoop => "single",
            PlayMode::Shuffle => "shuffle",
            PlayMode::Sequence => "sequence",
        }
        .into();
        self.cfg.save();
        self.set_status(format!("播放模式: {}", self.play_mode.label()));
    }

    fn download_current(&mut self) {
        let Some(song) = self.current.clone() else {
            self.set_status("没有正在播放的歌曲");
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        let br = self.cfg.br;
        tokio::spawn(async move {
            let result = async {
                let client = client.lock().await;
                let http = client.http();
                let url = client.song_url(song.id, br).await?;
                let resp = http.get(&url).send().await?;
                let status = resp.status();
                let bytes = resp.bytes().await?;
                if !status.is_success() || bytes.is_empty() {
                    return Err(anyhow!("下载失败: HTTP {}", status));
                }
                Ok::<_, anyhow::Error>(bytes.to_vec())
            }
            .await;
            match result {
                Ok(bytes) => {
                    let dir = crate::api::data_dir().join("downloads");
                    let _ = std::fs::create_dir_all(&dir);
                    let safe: String = song
                        .name
                        .chars()
                        .filter(|c| {
                            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                        })
                        .collect();
                    let path = dir.join(format!("{} - {}.mp3", song.artist_names(), safe));
                    match std::fs::write(&path, &bytes) {
                        Ok(()) => {
                            let _ = tx.send(Msg::DownloadDone(format!(
                                "已下载: {} ({} KB)",
                                path.display(),
                                bytes.len() / 1024
                            )));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::OpError(format!("保存文件失败: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Msg::OpError(format!("下载失败: {}", e)));
                }
            }
        });
    }

    fn persist_volume(&mut self) {
        self.cfg.volume = self.player.volume();
        self.cfg.save();
    }

    fn toggle_bitrate(&mut self) {
        self.cfg.br = if self.cfg.br <= 128000 {
            320000
        } else {
            128000
        };
        self.cfg.save();
        self.set_status(format!("音质: {}kbps", self.cfg.br / 1000));
    }

    // ---- message handling ----

    fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::PlaylistReady { id, name, songs } => {
                self.loading = false;
                if let View::PlaylistDetail {
                    id: v_id,
                    name: v_name,
                } = &mut self.view
                {
                    if *v_id == id {
                        *v_name = name;
                    }
                }
                self.playlist_songs = songs;
                self.set_status(format!("共 {} 首", self.playlist_songs.len()));
            }
            Msg::PlaylistListReady { items } => {
                self.playlists_loading = false;
                self.playlists = items;
                self.set_status(format!("共 {} 个歌单", self.playlists.len()));
            }
            Msg::ToplistReady { items } => {
                self.toplists_loading = false;
                self.toplists = items;
                self.set_status(format!("共 {} 个榜单", self.toplists.len()));
            }
            Msg::SquareReady { items } => {
                self.square_loading = false;
                self.square_playlists = items;
                self.set_status(format!(
                    "歌单广场 [{}] 共 {} 个精选歌单",
                    SQUARE_CATS[self.square_cat_index],
                    self.square_playlists.len()
                ));
            }
            Msg::SearchReady { songs } => {
                self.search_loading = false;
                self.search_results = songs;
                self.set_status(format!("找到 {} 首", self.search_results.len()));
            }
            Msg::PlaybackReady { song, bytes } => {
                self.loading_next = false;
                if self.current.as_ref().map(|s| s.id) == Some(song.id) {
                    match self.player.play_bytes(bytes) {
                        Ok(()) => {
                            self.set_status(format!(
                                "正在播放: {} - {}",
                                song.name,
                                song.artist_names()
                            ));
                        }
                        Err(e) => {
                            self.set_status(format!("播放失败: {}", e));
                        }
                    }
                }
            }
            Msg::LyricsReady { lines, trans } => {
                self.lyrics = lines;
                self.tlyric = trans;
            }
            Msg::DownloadDone(msg) => {
                self.set_status(msg);
            }
            Msg::OpError(e) => {
                self.loading = false;
                self.search_loading = false;
                self.playlists_loading = false;
                self.toplists_loading = false;
                self.loading_next = false;
                self.set_status(e);
            }
            Msg::QrKeyReady { key, qr } => {
                self.qr_unikey = Some(key);
                self.qr_string = Some(qr);
                self.qr_message = "请使用网易云音乐 App 扫码".into();
            }
            Msg::QrStatus { code, message } => {
                if code == 803 {
                    self.logged_in = true;
                    self.set_status("登录成功");
                    self.pop_view();
                } else if code == 800 {
                    self.qr_message = "二维码已过期，正在刷新...".into();
                    self.refresh_qr();
                } else {
                    self.qr_message = message;
                }
            }
            Msg::CookieLoginOk => {
                self.logged_in = true;
                self.set_status("登录成功 (cookie)");
                self.pop_view();
            }
            Msg::LoggedOut => {
                self.logged_in = false;
                self.set_status("已退出登录");
                self.go_main();
            }
        }
    }

    // ---- key handling ----

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quitting = true;
            return Ok(());
        }
        match &self.view {
            View::Login => self.handle_login_key(key),
            View::Search => self.handle_search_key(key),
            View::Player => self.handle_player_key(key),
            View::Main => self.handle_main_key(key),
            View::PlaylistList => self.handle_playlist_list_key(key),
            View::PlaylistDetail { .. } => self.handle_playlist_key(key),
            View::Queue => self.handle_queue_key(key),
            View::Help if key.code == KeyCode::Esc => {
                self.pop_view();
                Ok(())
            }
            View::Help => Ok(()),
            View::TopList => self.handle_toplists_key(key),
            View::Square => self.handle_square_key(key),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.main_index = self.main_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.main_index = (self.main_index + 1).min(6);
            }
            KeyCode::Enter => match self.main_index {
                0 => {
                    if !self.logged_in {
                        self.set_status("请先登录");
                    } else {
                        self.load_daily();
                    }
                }
                1 => self.load_playlists(),
                2 => self.load_square(),
                3 => self.load_toplists(),
                4 => {
                    self.search_input_mode = true;
                    self.push_view(View::Search);
                }
                5 => self.enter_login(),
                6 => {
                    self.quitting = true;
                }
                _ => {}
            },
            KeyCode::Esc => {
                if self.back_stack.is_empty() {
                    self.quitting = true;
                } else {
                    self.pop_view();
                }
            }
            KeyCode::Char('?') => {
                self.push_view(View::Help);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_playlist_list_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.playlists_index = self.playlists_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.playlists_index =
                    (self.playlists_index + 1).min(self.playlists.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(item) = self.playlists.get(self.playlists_index).cloned() {
                    self.open_playlist(item.id, item.name);
                }
            }
            KeyCode::Esc => {
                self.pop_view();
                self.playlists_loading = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_square_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.square_index = self.square_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.square_index =
                    (self.square_index + 1).min(self.square_playlists.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(item) = self.square_playlists.get(self.square_index).cloned() {
                    self.open_playlist(item.id, item.name);
                }
            }
            KeyCode::Char('c') => {
                self.square_cat_index = (self.square_cat_index + 1) % SQUARE_CATS.len();
                self.fetch_square();
            }
            KeyCode::Char('C') => {
                self.square_cat_index =
                    (self.square_cat_index + SQUARE_CATS.len() - 1) % SQUARE_CATS.len();
                self.fetch_square();
            }
            KeyCode::Esc => {
                self.pop_view();
                self.square_loading = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_toplists_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.toplists_index = self.toplists_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.toplists_index =
                    (self.toplists_index + 1).min(self.toplists.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(item) = self.toplists.get(self.toplists_index).cloned() {
                    self.open_playlist(item.id, item.name);
                }
            }
            KeyCode::Esc => {
                self.pop_view();
                self.toplists_loading = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_playlist_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.playlist_index = self.playlist_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.playlist_index =
                    (self.playlist_index + 1).min(self.playlist_songs.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                let songs = self.playlist_songs.clone();
                let idx = self.playlist_index;
                if !songs.is_empty() {
                    self.play_at(songs, idx);
                    self.push_view(View::Player);
                }
            }
            KeyCode::Esc => {
                self.pop_view();
                self.loading = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.search_input_mode {
            match key.code {
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Enter => {
                    self.search_input_mode = false;
                    self.start_search();
                }
                KeyCode::Esc => {
                    self.search_input_mode = false;
                }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.search_index = self.search_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.search_index =
                    (self.search_index + 1).min(self.search_results.len().saturating_sub(1));
            }
            KeyCode::Char('/') => {
                self.search_input_mode = true;
            }
            KeyCode::Enter => {
                let songs = self.search_results.clone();
                let idx = self.search_index;
                if !songs.is_empty() {
                    self.play_at(songs, idx);
                    self.push_view(View::Player);
                }
            }
            KeyCode::Esc => {
                self.pop_view();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_player_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char(' ') => self.player.toggle(),
            KeyCode::Char('s') => self.player.stop(),
            KeyCode::Char('n') | KeyCode::Right => self.next_song(),
            KeyCode::Char('p') | KeyCode::Left => self.prev_song(),
            KeyCode::Up => self
                .player
                .seek(self.player.position() + Duration::from_secs(5)),
            KeyCode::Down => {
                let pos = self.player.position();
                self.player.seek(pos.saturating_sub(Duration::from_secs(5)));
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.player.volume_up();
                self.persist_volume();
            }
            KeyCode::Char('-') => {
                self.player.volume_down();
                self.persist_volume();
            }
            KeyCode::Char('m') => self.toggle_play_mode(),
            KeyCode::Char('b') => self.toggle_bitrate(),
            KeyCode::Char('d') => self.download_current(),
            KeyCode::Char('v') => {
                self.queue_cursor = self.queue_index;
                self.push_view(View::Queue);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.pop_view();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_queue_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.queue_cursor = self.queue_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.queue_cursor = (self.queue_cursor + 1).min(self.queue.len().saturating_sub(1));
            }
            KeyCode::Enter if !self.queue.is_empty() => {
                self.queue_index = self.queue_cursor.min(self.queue.len() - 1);
                self.play_current();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.pop_view();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_login_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.login_mode = LoginMode::Qr;
                self.refresh_qr();
            }
            KeyCode::Char('c') => {
                self.login_mode = LoginMode::Cookie;
                self.qr_message = format!(
                    "将浏览器中的 MUSIC_U cookie 写入 {} 后按 r 读取",
                    Self::cookie_file_path().display()
                );
            }
            KeyCode::Char('r') => {
                if self.login_mode == LoginMode::Cookie {
                    self.login_from_cookie_file();
                } else {
                    self.refresh_qr();
                }
            }
            KeyCode::Esc => {
                self.pop_view();
            }
            _ => {}
        }
        Ok(())
    }

    // ---- main loop ----

    pub async fn run(mut self) -> Result<()> {
        let mut rx = self.rx.take().context("app already run")?;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();
        std::thread::spawn(move || loop {
            if let Ok(ev) = event::read() {
                let _ = event_tx.send(ev);
            }
        });

        let mut terminal = ratatui::init();
        let mut last_qr_poll = std::time::Instant::now();
        let mut last_frame = std::time::Instant::now();

        let result: Result<()> = async {
            loop {
                if self.quitting {
                    break;
                }

                // auto-advance when the current song finished
                if self.player.state() == PlayState::Playing
                    && self.player.ended()
                    && !self.loading_next
                {
                    self.next_song();
                }

                // QR polling every 1.2s while on the login page
                if matches!(self.view, View::Login)
                    && last_qr_poll.elapsed() > Duration::from_millis(1200)
                {
                    last_qr_poll = std::time::Instant::now();
                    self.poll_qr();
                }

                // lyric index update
                if !self.lyrics.is_empty() {
                    let idx =
                        lyric::current_index(&self.lyrics, self.player.position()).unwrap_or(0);
                    self.lyric_index = idx;
                }

                // render at ~30fps
                if last_frame.elapsed() > Duration::from_millis(33) {
                    last_frame = std::time::Instant::now();
                    terminal.draw(|f| self.render(f))?;
                }

                tokio::select! {
                    ev = event_rx.recv() => {
                        if let Some(Event::Key(key)) = ev {
                            self.handle_key(key)?;
                        }
                    }
                    msg = rx.recv() => {
                        if let Some(msg) = msg {
                            self.handle_msg(msg);
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
            Ok(())
        }
        .await;

        ratatui::restore();
        result
    }

    // ---- rendering ----

    fn render(&mut self, frame: &mut Frame) {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(frame.area());
        match &self.view {
            View::Main => self.render_main(frame, areas[0]),
            View::PlaylistList => self.render_playlist_list(frame, areas[0]),
            View::Square => self.render_square(frame, areas[0]),
            View::TopList => self.render_toplists(frame, areas[0]),
            View::PlaylistDetail { .. } => self.render_playlist(frame, areas[0]),
            View::Search => self.render_search(frame, areas[0]),
            View::Player => self.render_player(frame, areas[0]),
            View::Queue => self.render_queue(frame, areas[0]),
            View::Help => self.render_help(frame, areas[0]),
            View::Login => self.render_login(frame, areas[0]),
        }
        let status = Paragraph::new(Line::from(Span::styled(
            self.status_msg.clone(),
            Style::default().fg(Color::Green),
        )))
        .block(Block::default().borders(Borders::TOP));
        frame.render_widget(status, areas[1]);
    }

    fn render_main(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = [0usize, 1, 2, 3, 4, 5, 6]
            .iter()
            .map(|i| {
                let item = match i {
                    0 => MainItem::Daily,
                    1 => MainItem::Personalized,
                    2 => MainItem::Square,
                    3 => MainItem::Toplist,
                    4 => MainItem::Search,
                    5 => MainItem::Login,
                    _ => MainItem::Quit,
                };
                ListItem::new(Line::from(Span::styled(
                    item.label(self.logged_in),
                    Style::default(),
                )))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" rust-musicfox "),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.main_index));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_square(&mut self, frame: &mut Frame, area: Rect) {
        let cat = SQUARE_CATS[self.square_cat_index];
        let title = if self.square_loading {
            format!("歌单广场 [{}] (加载中...)", cat)
        } else {
            format!("歌单广场 [{}]  c 切换分类", cat)
        };
        let items: Vec<ListItem> = self
            .square_playlists
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let plays = p
                    .play_count
                    .map(|c| format!("  {}次播放", c))
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(p.name.clone()),
                    Span::styled(plays, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.square_index));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_toplists(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.toplists_loading {
            "榜单 (加载中...)".to_string()
        } else {
            "榜单".to_string()
        };
        let items: Vec<ListItem> = self
            .toplists
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let freq = t
                    .update_frequency
                    .clone()
                    .map(|f| format!("  [{}]", f))
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(t.name.clone()),
                    Span::styled(freq, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.toplists_index));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_playlist_list(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.playlists_loading {
            "推荐歌单 (加载中...)".to_string()
        } else {
            "推荐歌单".to_string()
        };
        let items: Vec<ListItem> = self
            .playlists
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let plays = p
                    .play_count
                    .map(|c| format!("  {}次播放", c as u64))
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(p.name.clone()),
                    Span::styled(plays, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.playlists_index));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_playlist(&mut self, frame: &mut Frame, area: Rect) {
        let name = match &self.view {
            View::PlaylistDetail { name, .. } => name.clone(),
            _ => return,
        };
        let title = if self.loading {
            format!("{} (加载中...)", name)
        } else {
            name
        };
        let items: Vec<ListItem> = self
            .playlist_songs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let dur = format_duration(s.duration_secs());
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(s.name.clone()),
                    Span::styled(
                        format!("  -  {}", s.artist_names()),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(format!("  [{}]", dur), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.playlist_index));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_search(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let input = Paragraph::new(if self.search_input_mode {
            format!("> {}", self.search_query)
        } else {
            format!("搜索 (按 / 输入): {}", self.search_query)
        })
        .block(Block::default().borders(Borders::ALL).title("搜索"))
        .style(if self.search_input_mode {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });
        frame.render_widget(input, chunks[0]);

        let items: Vec<ListItem> = self
            .search_results
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(s.name.clone()),
                    Span::styled(
                        format!("  -  {}", s.artist_names()),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(
                        format!("  [{}]", s.album_name()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(if self.search_loading {
                        "加载中..."
                    } else {
                        "结果"
                    }),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.search_index));
        frame.render_stateful_widget(list, chunks[1], &mut state);
    }

    fn render_player(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
            .split(area);

        let song = self.current.clone();
        let info = match &song {
            Some(s) => Line::from(vec![
                Span::styled(&s.name, Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" - {}", s.artist_names()),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled(
                    format!("  《{}》", s.album_name()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            None => Line::from("未在播放"),
        };
        frame.render_widget(
            Paragraph::new(info).block(Block::default().borders(Borders::ALL).title("正在播放")),
            chunks[0],
        );

        let total = song.as_ref().map(|s| s.duration_secs()).unwrap_or(0);
        let pos = self.player.position().as_secs();
        let ratio = if total > 0 {
            (pos as f64 / total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        frame.render_widget(
            Gauge::default()
                .block(Block::default().borders(Borders::ALL).title(format!(
                    "{} / {}  vol: {}%  [{}]  [{}]",
                    format_duration(pos),
                    format_duration(total),
                    (self.player.volume() * 100.0) as u32,
                    state_str(self.player.state()),
                    self.play_mode.label()
                )))
                .gauge_style(Style::default().fg(Color::Cyan))
                .ratio(ratio),
            chunks[1],
        );

        let mut lines: Vec<Line> = Vec::new();
        if self.lyrics.is_empty() {
            lines.push(Line::from("暂无歌词"));
        } else {
            let start = self.lyric_index.saturating_sub(2);
            let end = (self.lyric_index + 3).min(self.lyrics.len());
            for i in start..end {
                let line = &self.lyrics[i];
                let is_current = i == self.lyric_index;
                lines.push(Line::from(Span::styled(
                    line.text.clone(),
                    if is_current {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                )));
                // translated lyric under the current line (same timestamp)
                if is_current {
                    if let Some(tr) = self.tlyric.iter().find(|t| t.time_ms == line.time_ms) {
                        lines.push(Line::from(Span::styled(
                            tr.text.clone(),
                            Style::default().fg(Color::Yellow),
                        )));
                    }
                }
            }
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("歌词"))
                .wrap(Wrap { trim: true }),
            chunks[2],
        );

        frame.render_widget(
            Paragraph::new(Line::from(
                "空格 播放/暂停  s 停止  n/→ 下一首  p/← 上一首  ↑/↓ 快进快退5s  +/- 音量  m 模式  b 音质  d 下载  v 队列  q 返回",
            ))
            .style(Style::default().fg(Color::DarkGray)),
            chunks[3],
        );
    }

    fn render_queue(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .queue
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mark = if i == self.queue_index { "▶ " } else { "   " };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:>3}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(s.name.clone()),
                    Span::styled(
                        format!("  -  {}", s.artist_names()),
                        Style::default().fg(Color::Blue),
                    ),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("播放队列 ({})", self.queue.len())),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("> ");
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.queue_cursor));
        frame.render_stateful_widget(list, area, &mut state);
        frame.render_widget(
            Paragraph::new(Line::from("Enter 播放选中  Esc 返回"))
                .style(Style::default().fg(Color::DarkGray)),
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(area)[1],
        );
    }

    fn render_help(&mut self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(Span::styled(
                "全局",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  ↑/↓ 或 k/j    移动光标"),
            Line::from("  Enter           进入 / 播放"),
            Line::from("  Esc             返回上一页"),
            Line::from("  Ctrl+C          退出"),
            Line::from(""),
            Line::from(Span::styled(
                "主菜单",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  ?               帮助"),
            Line::from(""),
            Line::from(Span::styled(
                "歌单广场",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  c / C           切换分类 (下一/上一)"),
            Line::from(""),
            Line::from(Span::styled(
                "播放页",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  空格            播放 / 暂停"),
            Line::from("  s               停止"),
            Line::from("  n / →           下一首"),
            Line::from("  p / ←           上一首"),
            Line::from("  ↑ / ↓           快进 / 快退 5 秒"),
            Line::from("  + / -           音量加减"),
            Line::from("  m               播放模式 (列表循环/单曲/随机/顺序)"),
            Line::from("  b               音质切换 (128k/320k)"),
            Line::from("  d               下载当前歌曲"),
            Line::from("  v               播放队列"),
            Line::from("  q / Esc         返回"),
            Line::from(""),
            Line::from(Span::styled(
                "搜索页",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  /               进入输入"),
            Line::from("  Enter           搜索"),
            Line::from(""),
            Line::from(Span::styled(
                "登录页",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("  q               扫码模式"),
            Line::from("  c               Cookie 模式"),
            Line::from("  r               刷新二维码 / 读取 cookie"),
            Line::from(""),
            Line::from("按 Esc 返回"),
        ];
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title("帮助"))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_login(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(3)])
            .split(area);

        let mut text = Vec::new();
        if self.login_mode == LoginMode::Cookie {
            text.push(Line::from(""));
            text.push(Line::from(Span::styled(
                "Cookie 登录",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            text.push(Line::from(""));
            text.push(Line::from("1. 用浏览器打开 https://music.163.com 并登录"));
            text.push(Line::from(
                "2. 按 F12 打开开发者工具 → Application → Cookies",
            ));
            text.push(Line::from("3. 复制 MUSIC_U 的完整 cookie 字符串"));
            text.push(Line::from("   （格式: MUSIC_U=xxxxx; 其他=yyy 均可）"));
            text.push(Line::from(format!(
                "4. 写入文件: {}",
                Self::cookie_file_path().display()
            )));
            text.push(Line::from("5. 回到这里按 r 读取并登录"));
            text.push(Line::from(""));
            text.push(Line::from(Span::styled(
                self.qr_message.clone(),
                Style::default().fg(Color::Yellow),
            )));
        } else {
            if let Some(qr) = &self.qr_string {
                for (i, line) in qr.lines().enumerate() {
                    if i >= 20 {
                        break;
                    }
                    text.push(Line::from(line.to_string()));
                }
            } else {
                text.push(Line::from("二维码加载中..."));
            }
            text.push(Line::from(""));
            text.push(Line::from(Span::styled(
                self.qr_message.clone(),
                Style::default().fg(Color::Yellow),
            )));
        }

        let title = if self.login_mode == LoginMode::Cookie {
            "登录 (Cookie 模式)"
        } else {
            "扫码登录"
        };
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title(title))
                .alignment(Alignment::Center),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(
                "q 扫码模式  c Cookie 模式  r 刷新/读取  Esc 返回",
            ))
            .style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }
}

fn format_duration(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn state_str(s: PlayState) -> &'static str {
    match s {
        PlayState::Stopped => "停止",
        PlayState::Playing => "播放中",
        PlayState::Paused => "已暂停",
    }
}
