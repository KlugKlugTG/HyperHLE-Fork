/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Threaded FFmpeg demux + decode pipeline.
//!
//! All FFmpeg state (`AVFormatContext`, decoders, sws/swr contexts,
//! `AVFrame`s) lives on the worker thread. The consumer (host main loop)
//! only touches the bounded `sync_channel` receivers and a couple of
//! atomics, none of which can block.
//!
//! Decoded video is always converted to RGBA8 (one plane, possibly padded
//! stride). Decoded audio is always converted to interleaved S16 stereo at
//! the source sample rate.

use super::backend::{
    AudioChunk, AudioConfig, AudioSampleFormat, OpenMediaError, PixelFormat, PlayerEvent,
    VideoConfig, VideoFrame,
};
use ffmpeg::format::Pixel;
use ffmpeg::software::resampling::context::Context as SwrCtx;
use ffmpeg::software::scaling::{context::Context as SwsCtx, flag::Flags as SwsFlags};
use ffmpeg::util::channel_layout::ChannelLayout;
use ffmpeg::util::format::sample::{Sample, Type as SampleType};
use ffmpeg_next as ffmpeg;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TrySendError};
use std::sync::{Arc, Once};
use std::thread::JoinHandle;

/// How many decoded video frames to keep buffered. ~½ s at 30 fps. Bounded
/// to keep memory usage predictable and to apply natural backpressure on
/// the decoder.
const VIDEO_QUEUE_LEN: usize = 16;
/// How many decoded audio chunks to keep buffered. ≈ 0.5–1 s of audio.
const AUDIO_QUEUE_LEN: usize = 32;

/// Public output of [`open`]: handles the consumer can read from and an
/// owned worker thread.
pub(super) struct OpenedMedia {
    pub video_config: Option<VideoConfig>,
    pub audio_config: Option<AudioConfig>,
    pub duration_seconds: Option<f64>,
    pub video_rx: Option<Receiver<VideoFrame>>,
    pub audio_rx: Option<Receiver<AudioChunk>>,
    pub event_rx: Receiver<PlayerEvent>,
    pub stop_flag: Arc<AtomicBool>,
    pub worker: Option<JoinHandle<()>>,
}

static FFMPEG_INIT: Once = Once::new();
fn ffmpeg_init() -> Result<(), OpenMediaError> {
    let mut err: Option<String> = None;
    FFMPEG_INIT.call_once(|| {
        if let Err(e) = ffmpeg::init() {
            err = Some(e.to_string());
        }
    });
    if let Some(msg) = err {
        Err(OpenMediaError::Ffmpeg(msg))
    } else {
        Ok(())
    }
}

/// Open `path` and start the decoder thread.
pub(super) fn open(path: PathBuf) -> Result<OpenedMedia, OpenMediaError> {
    ffmpeg_init()?;

    if !path.exists() {
        return Err(OpenMediaError::NotFound);
    }

    // Probe metadata up-front so we can return useful errors synchronously.
    let probe = ffmpeg::format::input(&path).map_err(|e| OpenMediaError::Ffmpeg(e.to_string()))?;

    let duration_seconds = if probe.duration() > 0 {
        Some(probe.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64)
    } else {
        None
    };

    let video_meta = probe
        .streams()
        .best(ffmpeg::media::Type::Video)
        .and_then(|s| {
            let cctx = ffmpeg::codec::Context::from_parameters(s.parameters()).ok()?;
            let dec = cctx.decoder().video().ok()?;
            let fr = s.avg_frame_rate();
            let frame_rate = if fr.numerator() > 0 && fr.denominator() > 0 {
                fr.numerator() as f64 / fr.denominator() as f64
            } else {
                30.0
            };
            Some(VideoConfig {
                width: dec.width(),
                height: dec.height(),
                frame_rate,
                pixel_format: PixelFormat::Rgba8,
            })
        });

    let audio_meta = probe
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .and_then(|s| {
            let cctx = ffmpeg::codec::Context::from_parameters(s.parameters()).ok()?;
            let dec = cctx.decoder().audio().ok()?;
            Some(AudioConfig {
                sample_rate: dec.rate(),
                channels: 2,
                format: AudioSampleFormat::S16,
            })
        });

    drop(probe);

    if video_meta.is_none() && audio_meta.is_none() {
        return Err(OpenMediaError::UnsupportedFormat(
            "no audio or video stream".into(),
        ));
    }

    let (event_tx, event_rx) = sync_channel::<PlayerEvent>(8);
    let (video_tx, video_rx) = if video_meta.is_some() {
        let (t, r) = sync_channel::<VideoFrame>(VIDEO_QUEUE_LEN);
        (Some(t), Some(r))
    } else {
        (None, None)
    };
    let (audio_tx, audio_rx) = if audio_meta.is_some() {
        let (t, r) = sync_channel::<AudioChunk>(AUDIO_QUEUE_LEN);
        (Some(t), Some(r))
    } else {
        (None, None)
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_worker = stop_flag.clone();

    let worker = std::thread::Builder::new()
        .name("touchHLE_ffmpeg_decoder".into())
        .spawn(move || {
            let _ = event_tx.try_send(PlayerEvent::Started);
            let result = run_decode_loop(path, &stop_flag_worker, video_tx, audio_tx);
            match result {
                Ok(()) => {
                    let _ = event_tx.try_send(PlayerEvent::Finished);
                }
                Err(e) => {
                    let _ = event_tx.try_send(PlayerEvent::Failed(e));
                }
            }
        })
        .map_err(|e| OpenMediaError::Ffmpeg(format!("could not spawn decoder thread: {e}")))?;

    Ok(OpenedMedia {
        video_config: video_meta,
        audio_config: audio_meta,
        duration_seconds,
        video_rx,
        audio_rx,
        event_rx,
        stop_flag,
        worker: Some(worker),
    })
}

/// Sends `frame` to `tx`, but cooperatively bails out if the consumer has
/// gone away or the caller has requested a stop.
///
/// Returns `Ok(true)` if the frame was sent, `Ok(false)` if we should exit
/// the decode loop, `Err(_)` only on a genuine SendError (= receiver
/// dropped).
fn cooperative_send<T>(
    tx: &SyncSender<T>,
    mut payload: T,
    stop_flag: &AtomicBool,
) -> Result<bool, SendError<T>> {
    use std::time::Duration;
    loop {
        if stop_flag.load(Ordering::Acquire) {
            return Ok(false);
        }
        match tx.try_send(payload) {
            Ok(()) => return Ok(true),
            Err(TrySendError::Full(returned)) => {
                payload = returned;
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(TrySendError::Disconnected(_)) => {
                // Consumer dropped the receiver – e.g. backend::stop().
                return Ok(false);
            }
        }
    }
}

fn run_decode_loop(
    path: PathBuf,
    stop_flag: &AtomicBool,
    video_tx: Option<SyncSender<VideoFrame>>,
    audio_tx: Option<SyncSender<AudioChunk>>,
) -> Result<(), String> {
    let mut ictx = ffmpeg::format::input(&path).map_err(|e| format!("input open failed: {e}"))?;

    let video_idx = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .map(|s| s.index());
    let audio_idx = ictx
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .map(|s| s.index());

    // Decoder + colour converter for video.
    let mut video: Option<VideoDecode> = match video_idx {
        Some(idx) => Some(VideoDecode::new(&ictx, idx)?),
        None => None,
    };

    // Decoder + resampler for audio.
    let mut audio: Option<AudioDecode> = match audio_idx {
        Some(idx) => Some(AudioDecode::new(&ictx, idx)?),
        None => None,
    };

    // Demux loop. We rely on the sync_channel back-pressure to throttle
    // ourselves: cooperative_send will park briefly if the queue is full.
    for (stream, packet) in ictx.packets() {
        if stop_flag.load(Ordering::Acquire) {
            return Ok(());
        }

        let stream_idx = stream.index();
        if Some(stream_idx) == video_idx {
            if let (Some(v), Some(tx)) = (video.as_mut(), video_tx.as_ref()) {
                v.feed_packet(&packet, tx, stop_flag)?;
            }
        } else if Some(stream_idx) == audio_idx {
            if let (Some(a), Some(tx)) = (audio.as_mut(), audio_tx.as_ref()) {
                a.feed_packet(&packet, tx, stop_flag)?;
            }
        }
    }

    // Flush decoders.
    if let (Some(v), Some(tx)) = (video.as_mut(), video_tx.as_ref()) {
        v.flush(tx, stop_flag)?;
    }
    if let (Some(a), Some(tx)) = (audio.as_mut(), audio_tx.as_ref()) {
        a.flush(tx, stop_flag)?;
    }

    Ok(())
}

// -----------------------------------------------------------------------
// Video pipeline
// -----------------------------------------------------------------------

struct VideoDecode {
    decoder: ffmpeg::decoder::Video,
    scaler: SwsCtx,
    time_base: ffmpeg::Rational,
    out_w: u32,
    out_h: u32,
}

impl VideoDecode {
    fn new(ictx: &ffmpeg::format::context::Input, stream_idx: usize) -> Result<Self, String> {
        let stream = ictx.stream(stream_idx).ok_or("video stream index gone")?;
        let cctx = ffmpeg::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("video codec init: {e}"))?;
        let decoder = cctx
            .decoder()
            .video()
            .map_err(|e| format!("video decoder: {e}"))?;
        let scaler = SwsCtx::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            SwsFlags::BILINEAR,
        )
        .map_err(|e| format!("sws_getContext: {e}"))?;
        Ok(Self {
            time_base: stream.time_base(),
            out_w: decoder.width(),
            out_h: decoder.height(),
            decoder,
            scaler,
        })
    }

    fn feed_packet(
        &mut self,
        packet: &ffmpeg::Packet,
        tx: &SyncSender<VideoFrame>,
        stop_flag: &AtomicBool,
    ) -> Result<(), String> {
        if let Err(e) = self.decoder.send_packet(packet) {
            // EAGAIN / EOF are recoverable; drop and continue.
            log_dbg(&format!("video send_packet: {e}"));
        }
        self.drain(tx, stop_flag)
    }

    fn flush(&mut self, tx: &SyncSender<VideoFrame>, stop_flag: &AtomicBool) -> Result<(), String> {
        let _ = self.decoder.send_eof();
        self.drain(tx, stop_flag)
    }

    fn drain(&mut self, tx: &SyncSender<VideoFrame>, stop_flag: &AtomicBool) -> Result<(), String> {
        let mut decoded = ffmpeg::frame::Video::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let mut rgba = ffmpeg::frame::Video::empty();
            self.scaler
                .run(&decoded, &mut rgba)
                .map_err(|e| format!("sws_scale: {e}"))?;

            let stride = rgba.stride(0);
            let stride_bytes = if stride > 0 {
                stride as u32
            } else {
                self.out_w * 4
            };
            let plane = rgba.data(0);
            let needed = stride_bytes as usize * self.out_h as usize;
            let data = plane[..needed.min(plane.len())].to_vec();

            let pts = decoded.pts().or_else(|| decoded.timestamp()).unwrap_or(0);
            let pts_seconds = pts as f64 * self.time_base.numerator() as f64
                / self.time_base.denominator().max(1) as f64;

            let frame = VideoFrame {
                width: self.out_w,
                height: self.out_h,
                stride_bytes,
                pixel_format: PixelFormat::Rgba8,
                pts_seconds,
                data,
            };

            match cooperative_send(tx, frame, stop_flag) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(_) => return Ok(()),
            }
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------
// Audio pipeline
// -----------------------------------------------------------------------

struct AudioDecode {
    decoder: ffmpeg::decoder::Audio,
    resampler: SwrCtx,
    time_base: ffmpeg::Rational,
    out_rate: u32,
    out_channels: u16,
}

impl AudioDecode {
    fn new(ictx: &ffmpeg::format::context::Input, stream_idx: usize) -> Result<Self, String> {
        let stream = ictx.stream(stream_idx).ok_or("audio stream index gone")?;
        let cctx = ffmpeg::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("audio codec init: {e}"))?;
        let decoder = cctx
            .decoder()
            .audio()
            .map_err(|e| format!("audio decoder: {e}"))?;

        let in_layout = if decoder.channel_layout().bits() == 0 {
            ChannelLayout::default(decoder.channels() as i32)
        } else {
            decoder.channel_layout()
        };
        let out_layout = ChannelLayout::STEREO;
        let out_format = Sample::I16(SampleType::Packed);
        let out_rate = decoder.rate();

        let resampler = SwrCtx::get(
            decoder.format(),
            in_layout,
            decoder.rate(),
            out_format,
            out_layout,
            out_rate,
        )
        .map_err(|e| format!("swr_alloc: {e}"))?;

        Ok(Self {
            time_base: stream.time_base(),
            out_rate,
            out_channels: 2,
            decoder,
            resampler,
        })
    }

    fn feed_packet(
        &mut self,
        packet: &ffmpeg::Packet,
        tx: &SyncSender<AudioChunk>,
        stop_flag: &AtomicBool,
    ) -> Result<(), String> {
        if let Err(e) = self.decoder.send_packet(packet) {
            log_dbg(&format!("audio send_packet: {e}"));
        }
        self.drain(tx, stop_flag)
    }

    fn flush(&mut self, tx: &SyncSender<AudioChunk>, stop_flag: &AtomicBool) -> Result<(), String> {
        let _ = self.decoder.send_eof();
        self.drain(tx, stop_flag)
    }

    fn drain(&mut self, tx: &SyncSender<AudioChunk>, stop_flag: &AtomicBool) -> Result<(), String> {
        let mut decoded = ffmpeg::frame::Audio::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let mut resampled = ffmpeg::frame::Audio::empty();
            self.resampler
                .run(&decoded, &mut resampled)
                .map_err(|e| format!("swr_convert: {e}"))?;

            let nb_samples = resampled.samples();
            let channels = self.out_channels as usize;
            let bytes = nb_samples * channels * 2;
            let plane = resampled.data(0);
            let take = bytes.min(plane.len());
            let samples_bytes = plane[..take].to_vec();

            let pts = decoded.pts().or_else(|| decoded.timestamp()).unwrap_or(0);
            let pts_seconds = pts as f64 * self.time_base.numerator() as f64
                / self.time_base.denominator().max(1) as f64;

            let chunk = AudioChunk {
                sample_rate: self.out_rate,
                channels: self.out_channels,
                format: AudioSampleFormat::S16,
                pts_seconds,
                samples_bytes,
            };

            match cooperative_send(tx, chunk, stop_flag) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(_) => return Ok(()),
            }
        }
        Ok(())
    }
}

#[inline]
fn log_dbg(_msg: &str) {
    #[cfg(debug_assertions)]
    eprintln!("[ffmpeg_pipeline] {_msg}");
}
