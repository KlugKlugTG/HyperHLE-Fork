// SPDX-License-Identifier: MPL-2.0
//
// Standalone smoke test for the FFmpeg movie backend.
//
// Build with: `cargo run --example ffmpeg_smoke --features ffmpeg -- path/to/movie.mp4`
//
// Without `--features ffmpeg` the binary still compiles but only exercises
// the NullBackend, which is useful for verifying that the default build
// stays untouched.

use std::time::{Duration, Instant};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: ffmpeg_smoke <movie file>");
        std::process::exit(2);
    });

    let mut backend = touchHLE::media::open_movie(std::path::Path::new(&path));

    let video_cfg = backend.video_config();
    let audio_cfg = backend.audio_config();
    let duration = backend.duration_seconds();

    println!("Opened {path}");
    println!("  video : {video_cfg:?}");
    println!("  audio : {audio_cfg:?}");
    println!("  total : {duration:?}");

    backend.play();

    let mut video_frames = 0u64;
    let mut audio_chunks = 0u64;
    let mut last_pts_v = 0.0f64;
    let mut last_pts_a = 0.0f64;
    let mut finished = false;
    let mut failed: Option<String> = None;
    let started = Instant::now();
    // Hard cap to keep CI / interactive runs from hanging on long media.
    let cap = Duration::from_secs(30);

    while !finished && failed.is_none() && started.elapsed() < cap {
        while let Some(frame) = backend.try_next_video_frame() {
            video_frames += 1;
            last_pts_v = frame.pts_seconds;
        }
        while let Some(chunk) = backend.try_next_audio_chunk() {
            audio_chunks += 1;
            last_pts_a = chunk.pts_seconds;
        }
        while let Some(event) = backend.try_recv_event() {
            match event {
                touchHLE::media::PlayerEvent::Started => {}
                touchHLE::media::PlayerEvent::Finished => finished = true,
                touchHLE::media::PlayerEvent::Failed(msg) => failed = Some(msg),
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    println!("After {:.2}s wall clock:", started.elapsed().as_secs_f64());
    println!("  video frames decoded : {video_frames} (last pts {last_pts_v:.3}s)");
    println!("  audio chunks decoded : {audio_chunks} (last pts {last_pts_a:.3}s)");
    println!("  finished             : {finished}");
    if let Some(msg) = failed {
        println!("  failed               : {msg}");
        std::process::exit(1);
    }

    backend.stop();
}
