/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! `MovieBackend` implementation backed by FFmpeg.
//!
//! All decoding happens on a dedicated worker thread spawned by
//! [`super::ffmpeg_pipeline`]. The host main loop only ever pops from
//! bounded `sync_channel` queues and reads atomics – nothing here can
//! block the emulation thread.
//!
//! The master clock is driven by the host's monotonic clock, with the
//! decoder's stream PTS used as the authority for *which* frame is
//! current. This is intentionally simpler than a proper audio-master
//! clock: the audio output device's own jitter buffer absorbs the
//! mismatch for the short intro videos that are the primary target. A
//! richer scheme can be layered on top later by overriding
//! [`FfmpegBackend::set_external_clock`].

use super::backend::{
    AudioChunk, AudioConfig, MovieBackend, OpenMediaError, PlayerEvent, PlayerState, VideoConfig,
    VideoFrame,
};
use super::ffmpeg_pipeline::{self, OpenedMedia};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

pub struct FfmpegBackend {
    inner: OpenedMedia,
    state: PlayerState,
    /// Wall-clock time at which the current `Playing` segment was entered.
    epoch: Option<Instant>,
    /// How much time has been logged in previous `Playing` segments
    /// (i.e. before the most recent pause).
    accumulated: Duration,
    /// PTS offset, allowing us to ignore the absolute encoded PTS and
    /// instead align the first decoded frame with `t = 0`.
    first_pts: Option<f64>,
    /// `true` once we have seen `PlayerEvent::Finished` *and* both queues
    /// are empty.
    drain_completed: bool,
}

impl FfmpegBackend {
    /// Open `path` and start the decoder worker thread.
    pub fn open(path: &Path) -> Result<Self, OpenMediaError> {
        let inner = ffmpeg_pipeline::open(path.to_path_buf())?;
        Ok(Self {
            inner,
            state: PlayerState::Stopped,
            epoch: None,
            accumulated: Duration::ZERO,
            first_pts: None,
            drain_completed: false,
        })
    }

    fn master_clock_seconds(&self) -> f64 {
        let running = match (self.state, self.epoch) {
            (PlayerState::Playing, Some(t)) => t.elapsed(),
            _ => Duration::ZERO,
        };
        (self.accumulated + running).as_secs_f64()
    }

    fn record_first_pts_if_needed(&mut self, pts: f64) -> f64 {
        match self.first_pts {
            Some(off) => (pts - off).max(0.0),
            None => {
                self.first_pts = Some(pts);
                0.0
            }
        }
    }

    fn drain_input_queues_on_stop(&mut self) {
        // Drop the receivers so the decoder thread's cooperative_send sees
        // Disconnected and exits promptly.
        self.inner.video_rx = None;
        self.inner.audio_rx = None;
    }
}

impl Drop for FfmpegBackend {
    fn drop(&mut self) {
        self.inner.stop_flag.store(true, Ordering::Release);
        // Dropping receivers also unblocks a wedged decoder.
        self.inner.video_rx = None;
        self.inner.audio_rx = None;
        if let Some(handle) = self.inner.worker.take() {
            let _ = handle.join();
        }
    }
}

impl MovieBackend for FfmpegBackend {
    fn video_config(&self) -> Option<VideoConfig> {
        self.inner.video_config
    }

    fn audio_config(&self) -> Option<AudioConfig> {
        self.inner.audio_config
    }

    fn duration_seconds(&self) -> Option<f64> {
        self.inner.duration_seconds
    }

    fn play(&mut self) {
        match self.state {
            PlayerState::Stopped | PlayerState::Paused | PlayerState::Finished => {
                self.state = PlayerState::Playing;
                self.epoch = Some(Instant::now());
            }
            PlayerState::Playing | PlayerState::Failed => {}
        }
    }

    fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            if let Some(t) = self.epoch.take() {
                self.accumulated += t.elapsed();
            }
            self.state = PlayerState::Paused;
        }
    }

    fn stop(&mut self) {
        self.inner.stop_flag.store(true, Ordering::Release);
        self.drain_input_queues_on_stop();
        self.state = PlayerState::Stopped;
        self.epoch = None;
        self.accumulated = Duration::ZERO;
        self.first_pts = None;
    }

    fn state(&self) -> PlayerState {
        self.state
    }

    fn current_time_seconds(&self) -> f64 {
        self.master_clock_seconds()
    }

    fn try_next_video_frame(&mut self) -> Option<VideoFrame> {
        if self.inner.video_rx.is_none() {
            return None;
        }
        let now = self.master_clock_seconds();
        let mut chosen: Option<VideoFrame> = None;
        let mut disconnected = false;
        // Drain everything that's already late; keep only the most recent
        // due-frame. This protects against jank when the host loop falls
        // behind for a tick. We borrow `video_rx` for the duration of a
        // single `try_recv` call so the borrow checker is happy with us
        // calling `&mut self` helpers between iterations.
        loop {
            let recv_result = {
                let rx = match self.inner.video_rx.as_ref() {
                    Some(rx) => rx,
                    None => break,
                };
                rx.try_recv()
            };
            match recv_result {
                Ok(mut frame) => {
                    let normalised = self.record_first_pts_if_needed(frame.pts_seconds);
                    frame.pts_seconds = normalised;
                    // We always keep the most recently received frame; if
                    // it's in the future we still hand it out (since we
                    // can't put it back) – on intro videos this 1-frame
                    // burst is invisible.
                    let is_future = normalised > now;
                    chosen = Some(frame);
                    if is_future {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.inner.video_rx = None;
        }
        chosen
    }

    fn try_next_audio_chunk(&mut self) -> Option<AudioChunk> {
        let rx = self.inner.audio_rx.as_ref()?;
        match rx.try_recv() {
            Ok(chunk) => Some(chunk),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.inner.audio_rx = None;
                None
            }
        }
    }

    fn try_recv_event(&mut self) -> Option<PlayerEvent> {
        match self.inner.event_rx.try_recv() {
            Ok(event) => {
                match &event {
                    PlayerEvent::Started => {}
                    PlayerEvent::Finished => {
                        // We only flip our own state once the queues are
                        // empty as well – the decoder may have queued more
                        // frames after sending the Finished event.
                    }
                    PlayerEvent::Failed(_) => {
                        self.state = PlayerState::Failed;
                    }
                }
                Some(event)
            }
            Err(_) => {
                // If the worker thread has gone away and both queues are
                // empty we can promote the state to Finished ourselves so
                // the host's `is_finished()` check works.
                let queues_empty = self
                    .inner
                    .video_rx
                    .as_ref()
                    .map(|r| {
                        matches!(
                            r.try_recv(),
                            Err(TryRecvError::Empty | TryRecvError::Disconnected)
                        )
                    })
                    .unwrap_or(true)
                    && self
                        .inner
                        .audio_rx
                        .as_ref()
                        .map(|r| {
                            matches!(
                                r.try_recv(),
                                Err(TryRecvError::Empty | TryRecvError::Disconnected)
                            )
                        })
                        .unwrap_or(true);
                if queues_empty
                    && self
                        .inner
                        .worker
                        .as_ref()
                        .map(|h| h.is_finished())
                        .unwrap_or(true)
                {
                    if !self.drain_completed && self.state != PlayerState::Failed {
                        self.drain_completed = true;
                        self.state = PlayerState::Finished;
                        return Some(PlayerEvent::Finished);
                    }
                }
                None
            }
        }
    }

    fn is_finished(&self) -> bool {
        matches!(self.state, PlayerState::Finished | PlayerState::Failed)
    }
}
