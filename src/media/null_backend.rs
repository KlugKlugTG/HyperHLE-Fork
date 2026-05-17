/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! No-op `MovieBackend` used when the `ffmpeg` feature is disabled or the
//! FFmpeg backend failed to open a particular file.
//!
//! Mirrors touchHLE's historical "post `PlaybackDidFinish` after 150ms"
//! behaviour: callers that wait for the finish notification (Spore Origins,
//! Crash Bandicoot Nitro Kart 3D, RE4, NFSU, ...) keep working without an
//! actual codec implementation.

use super::backend::{
    AudioChunk, AudioConfig, MovieBackend, PlayerEvent, PlayerState, VideoConfig, VideoFrame,
};
use std::time::{Duration, Instant};

/// Trivial backend that "plays" instantly.
pub struct NullBackend {
    state: PlayerState,
    started_at: Option<Instant>,
    finish_after: Duration,
    finished_event_emitted: bool,
}

impl NullBackend {
    pub fn new() -> Self {
        Self {
            state: PlayerState::Stopped,
            started_at: None,
            finish_after: Duration::from_millis(150),
            finished_event_emitted: false,
        }
    }
}

impl Default for NullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MovieBackend for NullBackend {
    fn video_config(&self) -> Option<VideoConfig> {
        None
    }
    fn audio_config(&self) -> Option<AudioConfig> {
        None
    }
    fn duration_seconds(&self) -> Option<f64> {
        Some(self.finish_after.as_secs_f64())
    }

    fn play(&mut self) {
        if self.state != PlayerState::Playing {
            self.state = PlayerState::Playing;
            self.started_at = Some(Instant::now());
        }
    }

    fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            self.state = PlayerState::Paused;
        }
    }

    fn stop(&mut self) {
        self.state = PlayerState::Stopped;
        self.started_at = None;
        self.finished_event_emitted = false;
    }

    fn state(&self) -> PlayerState {
        self.state
    }

    fn current_time_seconds(&self) -> f64 {
        match self.started_at {
            Some(t) if self.state == PlayerState::Playing => t.elapsed().as_secs_f64(),
            _ => 0.0,
        }
    }

    fn try_next_video_frame(&mut self) -> Option<VideoFrame> {
        None
    }
    fn try_next_audio_chunk(&mut self) -> Option<AudioChunk> {
        None
    }

    fn try_recv_event(&mut self) -> Option<PlayerEvent> {
        if self.state == PlayerState::Playing && !self.finished_event_emitted {
            if let Some(t) = self.started_at {
                if t.elapsed() >= self.finish_after {
                    self.state = PlayerState::Finished;
                    self.finished_event_emitted = true;
                    return Some(PlayerEvent::Finished);
                }
            }
        }
        None
    }

    fn is_finished(&self) -> bool {
        matches!(self.state, PlayerState::Finished | PlayerState::Stopped)
            && self.finished_event_emitted
    }
}
