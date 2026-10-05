/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Built-in H.264 video decoding for `MPMoviePlayerController`.

use openh264::decoder::{Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;
use openh264::OpenH264API;
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use symphonia::core::codecs::video::well_known::{
    extra_data::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG, CODEC_ID_H264,
};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::TimeBase;

const MAX_QUEUED_FRAMES: usize = 4;
const MAX_FRAME_PIXELS: usize = 8_294_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovieVideoInfo {
    pub width: u32,
    pub height: u32,
}

pub struct MovieFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pts: f64,
}

struct QueueState {
    frames: VecDeque<MovieFrame>,
    stopped: bool,
}

struct SharedFrames {
    state: Mutex<QueueState>,
    changed: Condvar,
}

struct VideoTrackInfo {
    track_id: u32,
    time_base: TimeBase,
    nal_length_size: usize,
    parameter_sets: Vec<u8>,
    video: MovieVideoInfo,
}

pub struct MovieVideo {
    shared: Arc<SharedFrames>,
    worker: Option<JoinHandle<()>>,
    last_playback_time: Option<f64>,
}

fn video_track_info(movie_bytes: &[u8]) -> Result<VideoTrackInfo, String> {
    let stream = MediaSourceStream::new(
        Box::new(Cursor::new(movie_bytes.to_vec())),
        Default::default(),
    );
    let format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| format!("container probe failed: {error}"))?;

    let (track, video) = format
        .tracks()
        .iter()
        .find_map(|track| {
            let Some(CodecParameters::Video(video)) = track.codec_params.as_ref() else {
                return None;
            };
            (video.codec == CODEC_ID_H264).then_some((track, video))
        })
        .ok_or_else(|| "no H.264 video track".to_string())?;
    let width = u32::from(
        video
            .width
            .ok_or_else(|| "video width is missing".to_string())?,
    );
    let height = u32::from(
        video
            .height
            .ok_or_else(|| "video height is missing".to_string())?,
    );
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| "video dimensions overflow".to_string())?;
    if width == 0 || height == 0 || pixels > MAX_FRAME_PIXELS {
        return Err(format!("unsupported video dimensions {width}x{height}"));
    }

    let time_base = track
        .time_base
        .unwrap_or_else(|| TimeBase::try_from_recip(24).unwrap());
    let config = video
        .extra_data
        .iter()
        .find(|entry| entry.id == VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG)
        .ok_or_else(|| "AVC decoder configuration is missing".to_string())?;
    let (nal_length_size, parameter_sets) = avcc_to_annex_b(&config.data)?;

    Ok(VideoTrackInfo {
        track_id: track.id,
        time_base,
        nal_length_size,
        parameter_sets,
        video: MovieVideoInfo { width, height },
    })
}

fn avcc_to_annex_b(config: &[u8]) -> Result<(usize, Vec<u8>), String> {
    if config.len() < 7 || config[0] != 1 {
        return Err("invalid AVCDecoderConfigurationRecord".to_string());
    }

    let nal_length_size = usize::from(config[4] & 3) + 1;
    if nal_length_size == 3 {
        return Err("reserved AVC NAL length size".to_string());
    }

    let mut position = 6;
    let sequence_count = usize::from(config[5] & 0x1f);
    let mut parameter_sets = Vec::new();
    for _ in 0..sequence_count {
        let length = usize::from(u16::from_be_bytes([
            *config.get(position).ok_or("missing SPS length")?,
            *config.get(position + 1).ok_or("missing SPS length")?,
        ]));
        position += 2;
        let end = position
            .checked_add(length)
            .ok_or_else(|| "SPS length overflow".to_string())?;
        let sequence = config
            .get(position..end)
            .ok_or_else(|| "truncated SPS".to_string())?;
        parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
        parameter_sets.extend_from_slice(sequence);
        position = end;
    }

    let picture_count = usize::from(*config.get(position).ok_or("missing PPS count")?);
    position += 1;
    for _ in 0..picture_count {
        let length = usize::from(u16::from_be_bytes([
            *config.get(position).ok_or("missing PPS length")?,
            *config.get(position + 1).ok_or("missing PPS length")?,
        ]));
        position += 2;
        let end = position
            .checked_add(length)
            .ok_or_else(|| "PPS length overflow".to_string())?;
        let picture = config
            .get(position..end)
            .ok_or_else(|| "truncated PPS".to_string())?;
        parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
        parameter_sets.extend_from_slice(picture);
        position = end;
    }

    if parameter_sets.is_empty() {
        return Err("AVC configuration has no SPS/PPS".to_string());
    }
    Ok((nal_length_size, parameter_sets))
}

fn packet_to_annex_b(packet: &[u8], nal_length_size: usize) -> Result<(Vec<u8>, bool), String> {
    let mut position = 0;
    let mut has_picture = false;
    let mut annex_b = Vec::with_capacity(packet.len() + 16);
    while position < packet.len() {
        if packet.len() - position < nal_length_size {
            return Err("truncated NAL length".to_string());
        }
        let mut length = 0usize;
        for byte in &packet[position..position + nal_length_size] {
            length = length
                .checked_mul(256)
                .and_then(|length| length.checked_add(usize::from(*byte)))
                .ok_or_else(|| "NAL length overflow".to_string())?;
        }
        position += nal_length_size;
        if length == 0 {
            return Err("empty NAL unit".to_string());
        }
        let end = position
            .checked_add(length)
            .ok_or_else(|| "NAL length overflow".to_string())?;
        let nal = packet
            .get(position..end)
            .ok_or_else(|| "truncated NAL unit".to_string())?;
        has_picture |= (1..=5).contains(&(nal[0] & 0x1f));
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(nal);
        position = end;
    }
    Ok((annex_b, has_picture))
}

fn next_pts(pending: &mut Vec<f64>, fallback: f64) -> f64 {
    let Some((index, _)) = pending
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
    else {
        return fallback;
    };
    pending.remove(index)
}

fn make_frame(yuv: openh264::decoder::DecodedYUV<'_>, pts: f64) -> Result<MovieFrame, String> {
    let (width, height) = yuv.dimensions();
    let pixel_count = width
        .checked_mul(height)
        .filter(|pixels| *pixels <= MAX_FRAME_PIXELS)
        .ok_or_else(|| format!("decoded frame dimensions are too large: {width}x{height}"))?;
    let byte_count = pixel_count
        .checked_mul(4)
        .ok_or_else(|| "decoded RGBA frame size overflow".to_string())?;
    let mut rgba = vec![0; byte_count];
    yuv.write_rgba8(&mut rgba);

    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| "decoded RGBA row size overflow".to_string())?;
    let mut row = vec![0; row_bytes];
    for top in 0..height / 2 {
        let bottom = height - top - 1;
        let top_start = top * row_bytes;
        let bottom_start = bottom * row_bytes;
        row.copy_from_slice(&rgba[top_start..top_start + row_bytes]);
        rgba.copy_within(bottom_start..bottom_start + row_bytes, top_start);
        rgba[bottom_start..bottom_start + row_bytes].copy_from_slice(&row);
    }

    Ok(MovieFrame {
        rgba,
        width: u32::try_from(width).map_err(|_| "frame width overflow".to_string())?,
        height: u32::try_from(height).map_err(|_| "frame height overflow".to_string())?,
        pts,
    })
}

fn is_stopped(shared: &SharedFrames) -> bool {
    shared.state.lock().unwrap().stopped
}

fn queue_frame(shared: &SharedFrames, frame: MovieFrame) -> bool {
    let mut state = shared.state.lock().unwrap();
    while state.frames.len() >= MAX_QUEUED_FRAMES && !state.stopped {
        state = shared.changed.wait(state).unwrap();
    }
    if state.stopped {
        return false;
    }
    state.frames.push_back(frame);
    true
}

fn decode_movie(
    movie_bytes: Arc<[u8]>,
    shared: Arc<SharedFrames>,
    looping: bool,
    info: VideoTrackInfo,
) {
    loop {
        if is_stopped(&shared) {
            return;
        }
        let stream = MediaSourceStream::new(
            Box::new(Cursor::new(movie_bytes.to_vec())),
            Default::default(),
        );
        let mut format = match symphonia::default::get_probe().probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        ) {
            Ok(format) => format,
            Err(error) => {
                log!("MPMoviePlayerController video: container probe failed: {error}");
                return;
            }
        };
        // Flushing after every access unit breaks OpenH264 reference state for B-frame streams.
        let decoder_config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let mut decoder = match Decoder::with_api_config(OpenH264API::from_source(), decoder_config)
        {
            Ok(decoder) => decoder,
            Err(error) => {
                log!("MPMoviePlayerController video: OpenH264 init failed: {error}");
                return;
            }
        };
        if let Err(error) = decoder.decode(&info.parameter_sets) {
            log!("MPMoviePlayerController video: OpenH264 rejected AVC headers: {error}");
            return;
        }

        let mut pending_pts = Vec::new();
        let mut last_pts = None;
        let mut frame_duration = 1.0 / 24.0;
        let mut decoded_any = false;
        loop {
            if is_stopped(&shared) {
                return;
            }
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(SymphoniaError::DecodeError(error)) => {
                    log!("MPMoviePlayerController video: skipping malformed MP4 packet: {error}");
                    continue;
                }
                Err(SymphoniaError::ResetRequired) => {
                    log!("MPMoviePlayerController video: MP4 stream requested a decoder reset");
                    return;
                }
                Err(error) => {
                    log!("MPMoviePlayerController video: packet read failed: {error}");
                    return;
                }
            };
            if packet.track_id != info.track_id {
                continue;
            }

            let (annex_b, has_picture) = match packet_to_annex_b(&packet.data, info.nal_length_size)
            {
                Ok(sample) => sample,
                Err(error) => {
                    log!("MPMoviePlayerController video: invalid AVC sample: {error}");
                    return;
                }
            };
            if !has_picture {
                continue;
            }
            let pts = info.time_base.calc_time(packet.pts).map_or_else(
                || last_pts.map_or(0.0, |last| last + frame_duration),
                |time| time.as_secs_f64(),
            );
            pending_pts.push(pts);
            let decoded = match decoder.decode(&annex_b) {
                Ok(decoded) => decoded,
                Err(error) => {
                    let failed_pts = next_pts(&mut pending_pts, pts);
                    log!("MPMoviePlayerController video: OpenH264 decode failed at {failed_pts:.3}s: {error}");
                    return;
                }
            };
            let Some(yuv) = decoded else {
                continue;
            };
            let frame_pts = next_pts(&mut pending_pts, pts).max(0.0);
            let frame = match make_frame(yuv, frame_pts) {
                Ok(frame) => frame,
                Err(error) => {
                    log!("MPMoviePlayerController video: {error}");
                    return;
                }
            };
            if let Some(previous) = last_pts {
                if frame_pts > previous {
                    frame_duration = (frame_pts - previous).clamp(1.0 / 240.0, 1.0);
                }
            }
            last_pts = Some(frame_pts);
            if !decoded_any {
                log!("MPMoviePlayerController video: decoded first frame at {frame_pts:.3}s");
            }
            decoded_any = true;
            if !queue_frame(&shared, frame) {
                return;
            }
        }

        let delayed_frames = match decoder.flush_remaining() {
            Ok(frames) => frames,
            Err(error) => {
                log!("MPMoviePlayerController video: couldn't flush delayed H.264 frames: {error}");
                Vec::new()
            }
        };
        for yuv in delayed_frames {
            let fallback = last_pts.map_or(0.0, |pts| pts + frame_duration);
            let frame_pts = next_pts(&mut pending_pts, fallback).max(0.0);
            let frame = match make_frame(yuv, frame_pts) {
                Ok(frame) => frame,
                Err(error) => {
                    log!("MPMoviePlayerController video: {error}");
                    return;
                }
            };
            if let Some(previous) = last_pts {
                if frame_pts > previous {
                    frame_duration = (frame_pts - previous).clamp(1.0 / 240.0, 1.0);
                }
            }
            last_pts = Some(frame_pts);
            decoded_any = true;
            if !queue_frame(&shared, frame) {
                return;
            }
        }

        if !looping || !decoded_any {
            return;
        }
    }
}

impl MovieVideo {
    pub fn probe(movie_bytes: &[u8]) -> Option<MovieVideoInfo> {
        video_track_info(movie_bytes).ok().map(|info| info.video)
    }

    pub fn start(movie_bytes: &[u8], looping: bool) -> Option<MovieVideo> {
        let info = match video_track_info(movie_bytes) {
            Ok(info) => info,
            Err(error) => {
                log!("MPMoviePlayerController video: {error}");
                return None;
            }
        };
        let video_info = info.video;
        let shared = Arc::new(SharedFrames {
            state: Mutex::new(QueueState {
                frames: VecDeque::new(),
                stopped: false,
            }),
            changed: Condvar::new(),
        });
        let worker_shared = shared.clone();
        let worker_bytes: Arc<[u8]> = Arc::from(movie_bytes);
        let worker = match thread::Builder::new()
            .name("touchhle-movie-video".to_string())
            .spawn(move || decode_movie(worker_bytes, worker_shared, looping, info))
        {
            Ok(worker) => worker,
            Err(error) => {
                log!("MPMoviePlayerController video: couldn't start decoder thread: {error}");
                return None;
            }
        };
        log!(
            "MPMoviePlayerController video: decoding {}x{} H.264 movie with OpenH264 (looping: {})",
            video_info.width,
            video_info.height,
            looping
        );
        Some(MovieVideo {
            shared,
            worker: Some(worker),
            last_playback_time: None,
        })
    }

    pub fn take_frame(&mut self, playback_time: f64) -> Option<MovieFrame> {
        if !playback_time.is_finite() {
            return None;
        }
        let playback_time = playback_time.max(0.0);
        if let Some(previous) = self.last_playback_time {
            if playback_time + 0.5 < previous {
                let mut state = self.shared.state.lock().unwrap();
                state.frames.clear();
                self.shared.changed.notify_all();
            }
        }
        self.last_playback_time = Some(playback_time);

        let mut state = self.shared.state.lock().unwrap();
        let mut latest = None;
        while state
            .frames
            .front()
            .is_some_and(|frame| frame.pts <= playback_time + 0.01)
        {
            latest = state.frames.pop_front();
        }
        if latest.is_some() {
            self.shared.changed.notify_all();
        }
        latest
    }
}

impl Drop for MovieVideo {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.stopped = true;
            state.frames.clear();
        }
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{avcc_to_annex_b, packet_to_annex_b, MovieVideo, MovieVideoInfo};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn avcc_parameter_sets_are_converted_to_annex_b() {
        let config = [
            1, 0x64, 0, 0x15, 0xff, 0xe1, 0, 2, 0x67, 0x64, 1, 0, 2, 0x68, 0xee,
        ];
        let (length_size, headers) = avcc_to_annex_b(&config).unwrap();
        assert_eq!(length_size, 4);
        assert_eq!(headers, [0, 0, 0, 1, 0x67, 0x64, 0, 0, 0, 1, 0x68, 0xee]);
    }

    #[test]
    fn avcc_samples_are_converted_and_identify_picture_nals() {
        let packet = [0, 0, 0, 2, 0x65, 0xaa];
        let (annex_b, has_picture) = packet_to_annex_b(&packet, 4).unwrap();
        assert!(has_picture);
        assert_eq!(annex_b, [0, 0, 0, 1, 0x65, 0xaa]);
    }

    #[test]
    fn avcc_samples_support_short_length_fields() {
        let packet = [0, 2, 0x41, 0xaa];
        let (annex_b, has_picture) = packet_to_annex_b(&packet, 2).unwrap();
        assert!(has_picture);
        assert_eq!(annex_b, [0, 0, 0, 1, 0x41, 0xaa]);
    }

    #[test]
    fn invalid_avcc_sample_length_is_rejected() {
        assert!(packet_to_annex_b(&[0, 0, 0, 2, 0x65], 4).is_err());
    }

    #[test]
    fn bundled_h264_decoder_outputs_timestamped_frames_without_host_tools() {
        let bytes = include_bytes!("../../../tests/fixtures/h264-smoke.mp4");
        assert_eq!(
            MovieVideo::probe(bytes),
            Some(MovieVideoInfo {
                width: 32,
                height: 32,
            })
        );
        let mut movie = MovieVideo::start(bytes, false).expect("H.264 fixture should open");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut frame_count = 0;
        let mut last_pts = -1.0;
        while frame_count < 8 && Instant::now() < deadline {
            let playback_time = frame_count as f64 / 24.0;
            if let Some(frame) = movie.take_frame(playback_time) {
                assert_eq!((frame.width, frame.height), (32, 32));
                assert_eq!(frame.rgba.len(), 32 * 32 * 4);
                assert!(frame.pts > last_pts);
                last_pts = frame.pts;
                frame_count += 1;
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(
            frame_count, 8,
            "decoder did not deliver eight frames in time"
        );
    }

    #[test]
    fn bundled_h264_decoder_handles_b_frames_without_black_video() {
        let bytes = include_bytes!("../../../tests/fixtures/h264-bframes.mp4");
        assert_eq!(
            MovieVideo::probe(bytes),
            Some(MovieVideoInfo {
                width: 64,
                height: 48,
            })
        );
        let mut movie = MovieVideo::start(bytes, false).expect("B-frame H.264 fixture should open");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut frame_count = 0;
        let mut last_pts = -1.0;
        while frame_count < 48 && Instant::now() < deadline {
            let playback_time = frame_count as f64 / 24.0;
            if let Some(frame) = movie.take_frame(playback_time) {
                assert_eq!((frame.width, frame.height), (64, 48));
                assert!(frame.pts > last_pts, "non-monotonic B-frame timestamp");
                assert!(
                    frame
                        .rgba
                        .chunks_exact(4)
                        .any(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0),
                    "decoder produced a black frame"
                );
                last_pts = frame.pts;
                frame_count += 1;
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(frame_count, 48, "decoder did not deliver every B-frame");
    }
}
