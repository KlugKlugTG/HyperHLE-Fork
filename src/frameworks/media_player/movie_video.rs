/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Optional video output for `MPMoviePlayerController`.
//!
//! touchHLE doesn't ship a video decoder. If an `ffmpeg` executable is on the
//! host's `PATH`, we use it as a subprocess to decode the movie to raw RGBA
//! frames, which the player shows in its `view`'s layer. Some apps build their
//! UI on top of a movie (e.g. BioShock's main menu is a looping video with
//! transparent buttons over it), so without this the screen stays black.
//! Without `ffmpeg`, nothing changes: the player behaves as before, minus the
//! picture.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// A running decoder. The decoded frame is flipped vertically (bottom row
/// first), which is the row order the compositor expects for raw pixels.
pub struct MovieVideo {
    child: Child,
    latest_frame: Arc<Mutex<Option<Vec<u8>>>>,
    pub width: u32,
    pub height: u32,
    temp_file: PathBuf,
}

impl MovieVideo {
    /// Start decoding `movie_bytes` in real time. Returns `None` (after
    /// logging why) if `ffmpeg`/`ffprobe` are unavailable or the movie can't
    /// be read.
    pub fn start(movie_bytes: &[u8], looping: bool) -> Option<MovieVideo> {
        static TEMP_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_file = std::env::temp_dir().join(format!(
            "touchHLE_movie_{}_{}.mp4",
            std::process::id(),
            n
        ));
        if let Err(e) = std::fs::write(&temp_file, movie_bytes) {
            log!("MPMoviePlayerController video: couldn't write temp file: {}", e);
            return None;
        }

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&temp_file)
            .stderr(Stdio::null())
            .output();
        let Some((width, height)) = probe.ok().and_then(|out| {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let (w, h) = text.split_once(',')?;
            Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
        }) else {
            log_once!(
                "MPMoviePlayerController video: ffprobe/ffmpeg not found on PATH or the movie has no video stream; movies will not be displayed."
            );
            let _ = std::fs::remove_file(&temp_file);
            return None;
        };

        let mut command = Command::new("ffmpeg");
        command.args(["-loglevel", "error", "-nostdin", "-re"]);
        if looping {
            command.args(["-stream_loop", "-1"]);
        }
        command
            .arg("-i")
            .arg(&temp_file)
            .args(["-an", "-vf", "vflip", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                log!("MPMoviePlayerController video: couldn't start ffmpeg: {}", e);
                let _ = std::fs::remove_file(&temp_file);
                return None;
            }
        };

        let latest_frame = Arc::new(Mutex::new(None));
        let mut stdout = child.stdout.take().unwrap();
        let frame_size = (width * height * 4) as usize;
        let sink = latest_frame.clone();
        std::thread::spawn(move || {
            let mut buffer = vec![0u8; frame_size];
            while stdout.read_exact(&mut buffer).is_ok() {
                *sink.lock().unwrap() = Some(buffer.clone());
            }
        });

        log!(
            "MPMoviePlayerController video: decoding {}x{} movie with ffmpeg (looping: {})",
            width,
            height,
            looping
        );
        Some(MovieVideo {
            child,
            latest_frame,
            width,
            height,
            temp_file,
        })
    }

    /// Take the most recent frame, if a new one was decoded since last time.
    pub fn take_frame(&self) -> Option<Vec<u8>> {
        self.latest_frame.lock().unwrap().take()
    }
}

impl Drop for MovieVideo {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.temp_file);
    }
}
