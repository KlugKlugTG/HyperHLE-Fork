/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Common types shared by every movie backend implementation.
//!
//! All structures here are intentionally plain old data (no Drop, no
//! interior mutability), with the exception of the buffers themselves which
//! are owned `Vec<u8>` / `Vec<i16>`. They are designed to cross thread
//! boundaries cheaply.

use std::fmt;

/// Pixel layout for [`VideoFrame::data`].
///
/// The FFmpeg backend always converts decoded video to [`PixelFormat::Rgba8`]
/// before handing it to the consumer, so the rest of the emulator does not
/// need a colour-space converter of its own.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8 bits per channel, R-G-B-A byte order, no premultiplication.
    Rgba8,
}

/// Sample layout for [`AudioChunk::samples_bytes`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AudioSampleFormat {
    /// 16-bit signed integer, host-endian, interleaved.
    S16,
    /// 32-bit float in `[-1.0, 1.0]`, host-endian, interleaved.
    F32,
}

impl AudioSampleFormat {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            AudioSampleFormat::S16 => 2,
            AudioSampleFormat::F32 => 4,
        }
    }
}

/// Description of the video stream a backend will deliver.
#[derive(Copy, Clone, Debug)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    /// Nominal frame rate in Hz, used for buffer sizing only. The real
    /// presentation timestamps come from each [`VideoFrame::pts_seconds`].
    pub frame_rate: f64,
    pub pixel_format: PixelFormat,
}

/// Description of the audio stream a backend will deliver.
#[derive(Copy, Clone, Debug)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: AudioSampleFormat,
}

/// One decoded video frame, ready to upload to a GPU texture.
///
/// `data` is always in `pixel_format` layout, of length
/// `stride_bytes * height`. `stride_bytes` is typically `width * 4` for
/// [`PixelFormat::Rgba8`] but callers must not assume that — FFmpeg's
/// `swscale` may pad each row.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixel_format: PixelFormat,
    pub pts_seconds: f64,
    pub data: Vec<u8>,
}

/// One chunk of decoded audio samples.
///
/// `samples_bytes` always contains a whole number of frames (a frame =
/// `channels` interleaved samples). The chunk size is decided by the decoder
/// and is typically equal to one decoded packet (≈ a few tens of ms).
pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: AudioSampleFormat,
    pub pts_seconds: f64,
    pub samples_bytes: Vec<u8>,
}

impl AudioChunk {
    /// Total number of interleaved samples (i.e. frames * channels).
    pub fn sample_count(&self) -> usize {
        self.samples_bytes.len() / self.format.bytes_per_sample()
    }

    /// Duration of this chunk in seconds, assuming the declared sample rate.
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }
        let frames = self.sample_count() as f64 / self.channels as f64;
        frames / self.sample_rate as f64
    }
}

/// Coarse player state, useful for logging and the
/// `playbackState` Objective-C property.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
    Finished,
    Failed,
}

/// One-shot notifications emitted by the backend's worker thread.
#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Started,
    Finished,
    Failed(String),
}

/// Errors that can occur while opening a media file.
#[derive(Debug)]
pub enum OpenMediaError {
    NotFound,
    UnsupportedFormat(String),
    Ffmpeg(String),
}

impl fmt::Display for OpenMediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenMediaError::NotFound => f.write_str("media file not found"),
            OpenMediaError::UnsupportedFormat(s) => write!(f, "unsupported format: {s}"),
            OpenMediaError::Ffmpeg(s) => write!(f, "ffmpeg error: {s}"),
        }
    }
}

impl std::error::Error for OpenMediaError {}

/// The public surface every movie backend implements.
///
/// All methods are non-blocking from the caller's point of view: the
/// expensive demux/decode/resample work happens on a worker thread owned by
/// the backend implementation. The host (emulator main loop) only ever
/// performs cheap channel pops and clock comparisons.
///
/// `Send`-only on purpose: the backend lives behind a `Box<dyn MovieBackend +
/// Send>` and is moved between the integration site and (potentially) other
/// host threads. It is not required to be `Sync`.
pub trait MovieBackend: Send {
    /// Stream metadata; `None` if there is no such stream.
    fn video_config(&self) -> Option<VideoConfig>;
    fn audio_config(&self) -> Option<AudioConfig>;

    /// Container duration in seconds, if known.
    fn duration_seconds(&self) -> Option<f64>;

    /// Begin or resume playback. Idempotent.
    fn play(&mut self);
    /// Pause playback. Idempotent. The internal clock stops advancing.
    fn pause(&mut self);
    /// Stop playback, flush queues, kill the worker thread. Idempotent.
    fn stop(&mut self);

    /// Current coarse state.
    fn state(&self) -> PlayerState;

    /// Master clock position in seconds since the start of playback.
    ///
    /// Backed by the audio clock when there is audio, by a monotonic
    /// host clock otherwise.
    fn current_time_seconds(&self) -> f64;

    /// Try to fetch a video frame whose presentation timestamp is at or
    /// before the master clock (i.e. one that should be on screen right
    /// now). Returns `None` if no frame is ready yet.
    fn try_next_video_frame(&mut self) -> Option<VideoFrame>;

    /// Try to fetch the next chunk of decoded audio samples. Returns `None`
    /// if the audio queue is empty (decoder is behind or stream has ended).
    fn try_next_audio_chunk(&mut self) -> Option<AudioChunk>;

    /// Try to receive a one-shot lifecycle event. Returns `None` if nothing
    /// is pending. The caller is expected to drain this on every tick of the
    /// host loop.
    fn try_recv_event(&mut self) -> Option<PlayerEvent>;

    /// `true` once the worker thread has signalled end-of-stream and both
    /// queues have been drained.
    fn is_finished(&self) -> bool;
}
