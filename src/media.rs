/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Multimedia (movie playback) backend layer.
//!
//! This module isolates the emulator core from any specific media decoding
//! library. The rest of the codebase only talks to the [`MovieBackend`] trait
//! and the small set of POD types declared in [`backend`].
//!
//! Two implementations ship with the repository:
//!
//! * [`null_backend::NullBackend`] – the default, always-compiled stub. It
//!   pretends a movie reached its end immediately and is the moral equivalent
//!   of touchHLE's historical "fake playback finished after 150ms" behaviour.
//!   Picked at runtime whenever the `ffmpeg` Cargo feature is disabled or the
//!   FFmpeg backend fails to open a particular movie file.
//! * [`ffmpeg_backend::FfmpegBackend`] – the real implementation. Demuxes and
//!   decodes media in a dedicated worker thread, exposes lock-free bounded
//!   queues of decoded RGBA video frames and interleaved PCM audio chunks, and
//!   keeps an A/V clock so the host can pull frames synchronously to wall
//!   time. Only compiled when the `ffmpeg` Cargo feature is enabled.
//!
//! Both implementations share the same [`MovieBackend`] surface, so the
//! integration point in `frameworks::media_player::movie_player` does not need
//! to know which one it is talking to.

pub mod backend;
pub mod null_backend;

#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_backend;
#[cfg(feature = "ffmpeg")]
mod ffmpeg_pipeline;

pub use backend::{
    AudioChunk, AudioConfig, AudioSampleFormat, MovieBackend, OpenMediaError, PlayerEvent,
    PlayerState, VideoConfig, VideoFrame,
};
pub use null_backend::NullBackend;

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_backend::FfmpegBackend;

use std::path::Path;

/// Open `path` with the best available backend.
///
/// The selection rule is simple:
///
/// * If the `ffmpeg` feature is enabled and the FFmpeg backend can open the
///   file, use it.
/// * Otherwise, return a [`NullBackend`] that pretends playback finished
///   immediately. This preserves touchHLE's historical "skip the cut-scene"
///   behaviour and lets the calling code stay format-agnostic.
pub fn open_movie(path: &Path) -> Box<dyn MovieBackend + Send> {
    #[cfg(feature = "ffmpeg")]
    {
        match FfmpegBackend::open(path) {
            Ok(b) => return Box::new(b),
            Err(e) => {
                log!(
                    "media::open_movie: FFmpeg backend could not open {:?}: {}; \
                     falling back to NullBackend",
                    path,
                    e
                );
            }
        }
    }
    Box::new(NullBackend::new())
}
