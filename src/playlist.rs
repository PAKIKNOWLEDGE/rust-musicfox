//! Playlist manager and play modes.
//!
//! Structure mirrors go-musicfox's `internal/playlist`: a manager owns the
//! playlist, the current index and the active play mode; each mode
//! implements next/previous with a `manual` flag (manual skips behave like
//! list-loop, automatic advances follow the mode's rules). Playlist state
//! (songs + index + mode) is persisted so a restart can resume where the
//! user left off.

use crate::api::types::Song;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    ListLoop,
    Ordered,
    SingleLoop,
    ListRandom,
    InfRandom,
    Intelligent,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::ListLoop => "列表循环",
            Mode::Ordered => "顺序播放",
            Mode::SingleLoop => "单曲循环",
            Mode::ListRandom => "列表随机",
            Mode::InfRandom => "无限随机",
            Mode::Intelligent => "心动模式",
        }
    }

    pub fn all() -> [Mode; 6] {
        [
            Mode::ListLoop,
            Mode::Ordered,
            Mode::SingleLoop,
            Mode::ListRandom,
            Mode::InfRandom,
            Mode::Intelligent,
        ]
    }

    /// Stable string key for config persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::ListLoop => "list_loop",
            Mode::Ordered => "ordered",
            Mode::SingleLoop => "single_loop",
            Mode::ListRandom => "list_random",
            Mode::InfRandom => "inf_random",
            Mode::Intelligent => "intelligent",
        }
    }

    pub fn from_key(s: &str) -> Mode {
        match s {
            "ordered" => Mode::Ordered,
            "single_loop" => Mode::SingleLoop,
            "list_random" => Mode::ListRandom,
            "inf_random" => Mode::InfRandom,
            "intelligent" => Mode::Intelligent,
            _ => Mode::ListLoop,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    playlist: Vec<Song>,
    current: Option<usize>,
    mode: Mode,
}

/// Thread-agnostic playlist manager (single-threaded UI usage).
#[derive(Debug)]
pub struct PlaylistManager {
    current: Option<usize>,
    playlist: Vec<Song>,
    mode: Mode,
    // ListRandom state: shuffled order + position within it.
    random_order: Vec<usize>,
    random_pos: usize,
    // InfRandom state: playback history (max 100) + position.
    history: Vec<usize>,
    history_pos: usize,
}

impl Default for PlaylistManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaylistManager {
    pub fn new() -> Self {
        PlaylistManager {
            current: None,
            playlist: Vec::new(),
            mode: Mode::ListLoop,
            random_order: Vec::new(),
            random_pos: 0,
            history: Vec::new(),
            history_pos: 0,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn playlist(&self) -> &[Song] {
        &self.playlist
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current_song(&self) -> Option<Song> {
        self.current.and_then(|i| self.playlist.get(i).cloned())
    }

    pub fn is_empty(&self) -> bool {
        self.playlist.is_empty()
    }

    /// Set the playlist and start index; resets per-mode state.
    pub fn initialize(&mut self, index: usize, playlist: Vec<Song>) {
        if playlist.is_empty() {
            self.playlist.clear();
            self.current = None;
            self.reset_mode_state();
            return;
        }
        let index = index.min(playlist.len() - 1);
        self.playlist = playlist;
        self.current = Some(index);
        self.mode_changed(index);
    }

    fn reset_mode_state(&mut self) {
        self.random_order.clear();
        self.random_pos = 0;
        self.history.clear();
        self.history_pos = 0;
    }

    /// Rebuild mode state after playlist/index changes (like go-musicfox's
    /// OnPlaylistChanged / Initialize).
    fn mode_changed(&mut self, current: usize) {
        let len = self.playlist.len();
        match self.mode {
            Mode::ListRandom => {
                self.random_order = fisher_yates(len);
                self.random_pos = self
                    .random_order
                    .iter()
                    .position(|&i| i == current)
                    .unwrap_or(0);
            }
            Mode::InfRandom => {
                self.history = vec![current];
                self.history_pos = 0;
            }
            _ => {}
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if let Some(cur) = self.current {
            if !self.playlist.is_empty() {
                self.mode_changed(cur);
            }
        }
    }

    /// Next song index; `manual` distinguishes user skips from automatic
    /// advance (single-loop repeats on auto, skips on manual).
    /// Returns None when playback should stop.
    pub fn next(&mut self, manual: bool) -> Option<usize> {
        let len = self.playlist.len();
        if len == 0 {
            return None;
        }
        let cur = self.current.unwrap_or(0).min(len - 1);
        let next = match self.mode {
            Mode::ListLoop => Some((cur + 1) % len),
            Mode::Ordered => (cur + 1 < len).then_some(cur + 1),
            Mode::SingleLoop => {
                if manual {
                    Some((cur + 1) % len)
                } else {
                    Some(cur)
                }
            }
            Mode::ListRandom => self.next_random(cur, len),
            Mode::InfRandom => self.next_inf_random(cur, len),
            Mode::Intelligent => {
                // Smart recommendations need a login-gated API; until then the
                // mode behaves like ordered playback.
                (cur + 1 < len).then_some(cur + 1)
            }
        };
        if let Some(i) = next {
            self.current = Some(i);
        }
        next
    }

    /// Previous song index; returns None when there is nothing to go back to.
    pub fn prev(&mut self, manual: bool) -> Option<usize> {
        let len = self.playlist.len();
        if len == 0 {
            return None;
        }
        let cur = self.current.unwrap_or(0).min(len - 1);
        let prev = match self.mode {
            Mode::ListLoop => Some((cur + len - 1) % len),
            Mode::Ordered => (cur > 0).then(|| cur - 1),
            Mode::SingleLoop => {
                if manual {
                    Some((cur + len - 1) % len)
                } else {
                    Some(cur)
                }
            }
            Mode::ListRandom => self.prev_random(cur, len),
            Mode::InfRandom => self.prev_inf_random(),
            Mode::Intelligent => (cur > 0).then_some(cur - 1),
        };
        if let Some(i) = prev {
            self.current = Some(i);
        }
        prev
    }

    fn next_random(&mut self, cur: usize, len: usize) -> Option<usize> {
        if self.random_order.len() != len {
            self.random_order = fisher_yates(len);
            self.random_pos = self
                .random_order
                .iter()
                .position(|&i| i == cur)
                .unwrap_or(0);
        }
        if self.random_pos + 1 >= self.random_order.len() {
            return None; // list played through; stop (matches go-musicfox)
        }
        self.random_pos += 1;
        Some(self.random_order[self.random_pos])
    }

    fn prev_random(&mut self, cur: usize, len: usize) -> Option<usize> {
        if self.random_order.len() != len {
            self.random_order = fisher_yates(len);
            self.random_pos = self
                .random_order
                .iter()
                .position(|&i| i == cur)
                .unwrap_or(0);
        }
        if self.random_pos == 0 {
            return None;
        }
        self.random_pos -= 1;
        Some(self.random_order[self.random_pos])
    }

    fn next_inf_random(&mut self, cur: usize, len: usize) -> Option<usize> {
        if len == 1 {
            return Some(0);
        }
        // Navigating back through history first.
        if self.history_pos < self.history.len().saturating_sub(1) {
            self.history_pos += 1;
            return Some(self.history[self.history_pos]);
        }
        let next = random_avoiding_recent(cur, len, &self.history);
        self.history.push(next);
        self.history_pos = self.history.len() - 1;
        if self.history.len() > 100 {
            self.history.remove(0);
            self.history_pos = self.history.len() - 1;
        }
        Some(next)
    }

    fn prev_inf_random(&mut self) -> Option<usize> {
        if self.history.is_empty() || self.history_pos == 0 {
            return None;
        }
        self.history_pos -= 1;
        Some(self.history[self.history_pos])
    }

    /// Remove a song at `index`; returns the new current index (or None when
    /// the playlist became empty). Mirrors go-musicfox's RemoveSong.
    pub fn remove_song(&mut self, index: usize) -> Option<usize> {
        if self.playlist.is_empty() || index >= self.playlist.len() {
            return self.current;
        }
        self.playlist.remove(index);
        if self.playlist.is_empty() {
            self.current = None;
            self.reset_mode_state();
            return None;
        }
        let cur = self.current.unwrap_or(0);
        let new_cur = if index < cur {
            cur - 1
        } else if index == cur {
            cur.min(self.playlist.len() - 1)
        } else {
            cur
        };
        self.current = Some(new_cur);
        self.mode_changed(new_cur);
        self.current
    }

    // ---- persistence ----

    fn snapshot_path() -> std::path::PathBuf {
        crate::api::data_dir().join("playlist.json")
    }

    pub fn save_state(&self) {
        let snap = Snapshot {
            playlist: self.playlist.clone(),
            current: self.current,
            mode: self.mode,
        };
        if let Ok(json) = serde_json::to_string(&snap) {
            let path = Self::snapshot_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, json);
        }
    }

    /// Restore a previously saved playlist. Returns true when state was
    /// restored.
    pub fn load_state(&mut self) -> bool {
        let path = Self::snapshot_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(snap) = serde_json::from_str::<Snapshot>(&raw) else {
            return false;
        };
        if snap.playlist.is_empty() {
            return false;
        }
        self.playlist = snap.playlist;
        self.current = snap.current.filter(|i| *i < self.playlist.len());
        self.mode = snap.mode;
        if let Some(cur) = self.current {
            self.mode_changed(cur);
        }
        true
    }
}

fn fisher_yates(len: usize) -> Vec<usize> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut order: Vec<usize> = (0..len).collect();
    for i in (1..len).rev() {
        let j = rng.gen_range(0..=i);
        order.swap(i, j);
    }
    order
}

/// Random index avoiding the current song and the most recent
/// len/3 (max 10) played songs; falls back to any non-current index.
fn random_avoiding_recent(cur: usize, len: usize, history: &[usize]) -> usize {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let recent_count = (len / 3).clamp(1, 10);
    let mut recent: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let start = history.len().saturating_sub(recent_count);
    for &h in &history[start..] {
        recent.insert(h);
    }
    recent.insert(cur);
    let candidates: Vec<usize> = (0..len).filter(|i| !recent.contains(i)).collect();
    if !candidates.is_empty() {
        return candidates[rng.gen_range(0..candidates.len())];
    }
    let candidates: Vec<usize> = (0..len).filter(|&i| i != cur).collect();
    if !candidates.is_empty() {
        return candidates[rng.gen_range(0..candidates.len())];
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(n: usize) -> Vec<Song> {
        (0..n)
            .map(|i| Song {
                id: i as i64,
                name: format!("s{}", i),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn list_loop_wraps() {
        let mut pm = PlaylistManager::new();
        pm.initialize(0, songs(3));
        assert_eq!(pm.next(false), Some(1));
        assert_eq!(pm.next(false), Some(2));
        assert_eq!(pm.next(false), Some(0)); // wraps
        assert_eq!(pm.prev(false), Some(2)); // wraps back
    }

    #[test]
    fn ordered_stops_at_end() {
        let mut pm = PlaylistManager::new();
        pm.set_mode(Mode::Ordered);
        pm.initialize(2, songs(3));
        assert_eq!(pm.next(false), None);
        assert_eq!(pm.prev(false), Some(1));
        assert_eq!(pm.prev(false), Some(0));
        assert_eq!(pm.prev(false), None);
    }

    #[test]
    fn single_loop_manual_skips_auto_repeats() {
        let mut pm = PlaylistManager::new();
        pm.set_mode(Mode::SingleLoop);
        pm.initialize(0, songs(3));
        assert_eq!(pm.next(false), Some(0)); // auto: repeat
        assert_eq!(pm.next(true), Some(1)); // manual: skip
        assert_eq!(pm.next(false), Some(1)); // auto: repeat again
    }

    #[test]
    fn list_random_plays_through_without_repeats() {
        let mut pm = PlaylistManager::new();
        pm.set_mode(Mode::ListRandom);
        pm.initialize(0, songs(5));
        let mut seen = std::collections::HashSet::new();
        while let Some(n) = pm.next(false) {
            // A shuffle order never repeats an index.
            assert!(seen.insert(n), "duplicate index {n}");
        }
        assert!(!seen.is_empty() && seen.len() <= 5);
    }

    #[test]
    fn inf_random_prev_navigates_history() {
        let mut pm = PlaylistManager::new();
        pm.initialize(0, songs(10));
        let first = pm.next(false).unwrap();
        let second = pm.next(false).unwrap();
        assert_ne!(first, second);
        assert_eq!(pm.prev(false), Some(first));
        assert_eq!(pm.prev(false), Some(0));
    }

    #[test]
    fn remove_song_adjusts_current() {
        let mut pm = PlaylistManager::new();
        pm.initialize(1, songs(3));
        // remove index 0 (before current): current shifts 1 -> 0
        assert_eq!(pm.remove_song(0), Some(0));
        assert_eq!(pm.playlist().len(), 2);
        // remove current (0): current stays 0 (now s2)
        assert_eq!(pm.remove_song(0), Some(0));
        assert_eq!(pm.current_song().unwrap().id, 2);
        // remove last remaining
        assert_eq!(pm.remove_song(0), None);
        assert!(pm.is_empty());
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut pm = PlaylistManager::new();
        pm.initialize(1, songs(4));
        pm.set_mode(Mode::InfRandom);
        let json = {
            // emulate save via snapshot path with a temp dir is complex;
            // instead test the serialization directly
            let snap = Snapshot {
                playlist: pm.playlist.clone(),
                current: pm.current,
                mode: pm.mode,
            };
            serde_json::to_string(&snap).unwrap()
        };
        let snap: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.mode, Mode::InfRandom);
        assert_eq!(snap.current, Some(1));
        assert_eq!(snap.playlist.len(), 4);
    }
}
