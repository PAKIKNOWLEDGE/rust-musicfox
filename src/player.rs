//! Audio playback via rodio (symphonia decoders: mp3/flac/ogg/wav).
//!
//! The player is a thin wrapper around `rodio::Sink`. The caller must
//! download the audio bytes (any thread) and hand them over through
//! `play_bytes`, which decodes and queues playback.

use std::io::Cursor;
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

pub struct Player {
    // Keep the output stream alive for the lifetime of the player.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
    state: PlayState,
    volume: f32,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (_stream, handle) =
            OutputStream::try_default().context("no audio output device available")?;
        Ok(Player {
            _stream,
            handle,
            sink: None,
            state: PlayState::Stopped,
            volume: 0.8,
        })
    }

    pub fn state(&self) -> PlayState {
        self.state
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Play raw audio bytes (mp3/flac/ogg/wav). Blocking decode; prefer
    /// calling from `spawn_blocking` when in an async context.
    pub fn play_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        let cursor = Cursor::new(bytes);
        let source = Decoder::new(cursor).context("audio decoder could not parse stream")?;
        let sink = Sink::try_new(&self.handle).context("create audio sink")?;
        sink.set_volume(self.volume);
        sink.append(source);
        if let Some(old) = self.sink.take() {
            old.stop();
        }
        self.sink = Some(sink);
        self.state = PlayState::Playing;
        Ok(())
    }

    /// True when playback finished or never started.
    pub fn ended(&self) -> bool {
        self.sink.as_ref().map(|s| s.empty()).unwrap_or(true)
    }

    pub fn pause(&mut self) {
        if let Some(sink) = &self.sink {
            sink.pause();
        }
        self.state = PlayState::Paused;
    }

    pub fn resume(&mut self) {
        if let Some(sink) = &self.sink {
            sink.play();
        }
        self.state = PlayState::Playing;
    }

    pub fn toggle(&mut self) {
        if self.state == PlayState::Paused {
            self.resume();
        } else if self.state == PlayState::Playing {
            self.pause();
        }
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.state = PlayState::Stopped;
    }

    pub fn seek(&mut self, pos: Duration) {
        if let Some(sink) = &self.sink {
            let _ = sink.try_seek(pos);
        }
    }

    /// Current playback position (only meaningful while playing/paused).
    pub fn position(&self) -> Duration {
        self.sink
            .as_ref()
            .map(|s| s.get_pos())
            .unwrap_or(Duration::ZERO)
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(sink) = &self.sink {
            sink.set_volume(self.volume);
        }
    }

    pub fn volume_up(&mut self) {
        self.set_volume(self.volume + 0.05);
    }

    pub fn volume_down(&mut self) {
        self.set_volume(self.volume - 0.05);
    }
}
