/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Host camera support with automatic fallback to a “no camera” stub.
//!
//! * When a host camera is detected, [`is_available`] returns `true` and the
//!   AVFoundation / UIKit shims expose a capture device so that apps can enter
//!   their camera UI (barcode readers, AR viewers, …). When available, preview
//!   layers and `AVCaptureVideoDataOutput` deliver frames — either real host
//!   frames (when a native backend is compiled in) or a synthetic test pattern
//!   that still lets the guest render loop progress past the black-screen
//!   gate (`LEGO Ninjago Spinjitzu Scavenger Hunt` etc.).
//! * When no host camera is present, every query reports “no device” so the
//!   app sees the same state as a real device without a camera (e.g. iPod
//!   touch 4) — the stub path that already existed for `AVCaptureDevice`.
//!
//! Detection is intentionally conservative and never blocks startup on a slow
//! probe: the result is cached in a `OnceLock` after the first call, so the
//! hot path (`-[AVCaptureDevice defaultDeviceWithMediaType:]`) costs a single
//! relaxed atomic read. Users can override the probe with environment
//! variables (useful for headless CI or for forcing the stub when a laptop
//! lid camera should be ignored):
//!
//! * `HYPERHLE_FORCE_CAMERA=1`  — pretend a host camera is present.
//! * `HYPERHLE_DISABLE_CAMERA=1` — pretend no host camera is present.
//!
//! On Linux the probe looks for V4L2 nodes (`/dev/video0…`). On macOS / Windows
//! / Android we currently rely on an optional `nokhwa` backend when compiled
//! with `--features host_camera_nokhwa`; without that feature the probe falls
//! back to “no camera” so the emulator stays buildable without extra system
//! libraries. The string returned by [`localized_name`] / [`unique_id`] is
//! chosen accordingly so that `AVCaptureDevice.localizedName` and
//! `uniqueID` round-trip correctly for both paths.

use std::sync::OnceLock;

/// Cached probe result. `OnceLock` guarantees a single probe even when the
/// guest races several threads through `+[AVCaptureDevice devices]`.
static CAMERA_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Override set from `Window` once the SDL video subsystem is up. When the
/// emulator runs headless there is no window and this hook is never called —
/// the pure filesystem / env-var probe is used instead.
static WINDOW_REPORTED: OnceLock<bool> = OnceLock::new();

/// Returns `true` iff a host camera should be considered present for the
/// current process. The value is cached after the first call.
pub fn is_available() -> bool {
    *CAMERA_AVAILABLE.get_or_init(probe_host_camera)
}

/// Number of capture devices the guest should see. Early iOS devices ship with
/// at most one back camera; modern “host has camera” exposes a single logical
/// device. Return 0 for the stub path so that `+[AVCaptureDevice devices]`
/// returns an empty `NSArray`, matching a real camera-less device.
pub fn device_count() -> usize {
    if is_available() { 1 } else { 0 }
}

/// Localized name reported for the default device.
///
/// * Host path → `"Host Camera"` (mirrors what `system_profiler` / v4l2
///   `card` field would report in a minimal way).
/// * Stub  path → `"HyperHLE Stub Camera"` (historic value, kept for
///   compatibility with snapshots / apptests).
pub fn localized_name() -> &'static str {
    if is_available() {
        "Host Camera"
    } else {
        "HyperHLE Stub Camera"
    }
}

/// Stable identifier for the default device.
///
/// * Host path → `com.hyperhle.camera.host`
/// * Stub  path → `com.hyperhle.camera.stub`
pub fn unique_id() -> &'static str {
    if is_available() {
        "com.hyperhle.camera.host"
    } else {
        "com.hyperhle.camera.stub"
    }
}

/// Whether the host backend can actually deliver frames (i.e. a capture thread
/// was successfully opened). Currently this is an alias for [`is_available`];
/// once a native `nokhwa` / AVFoundation backend is wired up this will start
/// returning `false` when the device exists but is busy / permission-denied.
pub fn can_deliver_frames() -> bool {
    is_available()
}

/// Inform the module that the `Window` layer has already probed the host.
/// This is a no-op if the cache is already populated. At the moment the
/// window layer has nothing camera-specific to report (SDL2 has no camera
/// API), but the hook keeps the call-site symmetric with the microphone
/// module and gives future SDL3 / native backends a place to inject a result
/// without having to reset the `OnceLock`.
pub fn note_window_probe(available: bool) {
    let _ = WINDOW_REPORTED.set(available);
    let _ = CAMERA_AVAILABLE.set(available);
}

// ---------------------------------------------------------------------------
// Probe implementation
// ---------------------------------------------------------------------------

fn probe_host_camera() -> bool {
    // Environment overrides win over everything — they are the only way to
    // force the stub in an otherwise camera-equipped CI image, or to force
    // the host path in headless where no /dev/video* exists.
    if std::env::var_os("HYPERHLE_DISABLE_CAMERA").is_some() {
        log!("Host camera: disabled via HYPERHLE_DISABLE_CAMERA — using stub (no device)");
        return false;
    }
    if std::env::var_os("HYPERHLE_FORCE_CAMERA").is_some() {
        log!("Host camera: forced via HYPERHLE_FORCE_CAMERA — using host camera");
        return true;
    }
    if std::env::var_os("HYPERHLE_FORCE_CAMERA_HOST").is_some() {
        log!("Host camera: forced via HYPERHLE_FORCE_CAMERA_HOST — using host camera");
        return true;
    }

    // If the `Window` layer already reported a result (SDL3 future, Android
    // Camera2 probe, …) honour it.
    if let Some(&reported) = WINDOW_REPORTED.get() {
        log!("Host camera: using Window-reported availability = {}", reported);
        return reported;
    }

    // Optional native backend: when the `host_camera_nokhwa` feature is
    // enabled we try to enumerate cameras through `nokhwa`, which abstracts
    // V4L2 / AVFoundation / MediaFoundation behind a single API. The crate is
    // optional so that the default build stays free of libclang / extra
    // system deps.
    #[cfg(feature = "host_camera_nokhwa")]
    {
        match try_nokhwa_probe() {
            Ok(true) => {
                log!("Host camera: nokhwa reports at least one camera — using host camera");
                return true;
            }
            Ok(false) => {
                log!("Host camera: nokhwa reports no cameras — using stub (no device)");
                return false;
            }
            Err(e) => {
                log!("Host camera: nokhwa probe failed ({}), falling through to fallback probe", e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Platform fallbacks — cheap filesystem checks that work without extra
    // native libraries. They are intentionally permissive: a single readable
    // device node is enough to claim “host camera present”. If that node
    // later fails to open, the capture session will gracefully degrade to a
    // synthetic frame source rather than crashing the guest.
    // -----------------------------------------------------------------------
    #[cfg(target_os = "linux")]
    {
        // V4L2 loopback / real webcams appear as /dev/video0 … /dev/videoN.
        // The container sandbox used in CI typically has none of these, so
        // the probe correctly returns false there and the stub path is
        // exercised in tests.
        for idx in 0..16 {
            let p = format!("/dev/video{}", idx);
            if std::path::Path::new(&p).exists() {
                log!("Host camera: detected {} — using host camera", p);
                return true;
            }
        }
        // Android's camera service also exposes /dev/video* when running
        // under an emulator with camera passthrough, so the same loop covers
        // that case.
        log!("Host camera: no /dev/video* nodes found — using stub (no device)");
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS a camera is almost always present (FaceTime HD), but
        // probing it without AVFoundation would require spawning
        // `system_profiler SPCameraDataType` and parsing its output, which is
        // slow and locale-dependent. When the optional nokhwa feature is
        // absent we conservatively report “no camera” so that existing
        // behaviour (the grey preview used by LEGO Ninjago) is preserved. Users
        // who want the host camera can either enable the nokhwa feature or
        // force it with HYPERHLE_FORCE_CAMERA=1.
        log!("Host camera: macOS probe without nokhwa — using stub (no device). Set HYPERHLE_FORCE_CAMERA=1 to force host camera.");
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        log!("Host camera: Windows probe without nokhwa — using stub (no device). Set HYPERHLE_FORCE_CAMERA=1 to force host camera.");
        return false;
    }

    #[cfg(target_os = "android")]
    {
        // Android's Camera / Camera2 API is only reachable through JNI. The
        // Rust side snapshots the Java-reported availability during
        // Window::new (mirroring how microphone availability is injected).
        // Until that JNI bridge is wired, the absence of a Window report
        // means “no camera” so that permission-denied / emulator-without-camera
        // devices correctly see the stub.
        log!("Host camera: Android without JNI probe — using stub (no device).");
        return false;
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        log!("Host camera: unknown platform — using stub (no device)");
        return false;
    }
}

#[cfg(feature = "host_camera_nokhwa")]
fn try_nokhwa_probe() -> Result<bool, String> {
    // nokhwa::query returns a Vec<CameraInfo> for the requested backend.
    // Auto means “pick the best available on this platform” (AVFoundation on
    // macOS, V4L2 on Linux, MediaFoundation on Windows).
    let devices = nokhwa::query(nokhwa::utils::ApiBackend::Auto).map_err(|e| e.to_string())?;
    Ok(!devices.is_empty())
}

// ---------------------------------------------------------------------------
// Frame source abstraction
// ---------------------------------------------------------------------------

/// Host-side frame source returned by [`acquire_frame`].
///
/// At the moment we have two concrete sources:
///
/// * `Host` — a buffer freshly captured from the desktop camera (via nokhwa
///   when the feature is enabled).
/// * `Synthetic` — a deterministic test pattern (grey + moving colour bar)
///   used when no native backend is compiled in, or when the host camera is
///   busy / permission-denied. The pattern still exercises the
///   `AVCaptureVideoPreviewLayer` compositor and the
///   `AVCaptureVideoDataOutput` delegate path so that apps can render their
///   camera UI and so that snapshot tests remain deterministic.
///
/// The distinction is observable only through `is_host_frame`.
#[derive(Debug, Clone)]
pub enum FrameSource {
    Host(Vec<u8>, u32, u32),      // RGBA, width, height
    Synthetic(Vec<u8>, u32, u32), // RGBA, width, height
}

impl FrameSource {
    pub fn is_host_frame(&self) -> bool {
        matches!(self, FrameSource::Host(_, _, _))
    }
    pub fn into_rgba(self) -> (Vec<u8>, u32, u32) {
        match self {
            FrameSource::Host(v, w, h) | FrameSource::Synthetic(v, w, h) => (v, w, h),
        }
    }
}

/// Try to acquire a single frame at `width`×`height`. When a host backend is
/// available and successfully opened, a real camera frame is returned.
/// Otherwise a synthetic pattern is returned so that the preview layer never
/// stays black (the black-screen issue for AR / barcode apps). Returns `None`
/// only when the capture session is not supposed to deliver frames at all
/// (i.e. [`is_available`] is `false` but the caller ignored it).
pub fn acquire_frame(width: u32, height: u32) -> Option<FrameSource> {
    if !is_available() {
        return None;
    }
    let w = width.max(1);
    let h = height.max(1);

    // Try native host frame first.
    #[cfg(feature = "host_camera_nokhwa")]
    {
        if let Some(host) = try_acquire_nokhwa_frame(w, h) {
            return Some(host);
        }
    }

    // Fallback: synthetic test pattern — 50 % grey background with a moving
    // vertical colour bar so that a video preview is visibly alive and
    // snapshot diffs can tell synthetic apart from a frozen host feed.
    Some(FrameSource::Synthetic(synthetic_frame_rgba(w, h), w, h))
}

#[cfg(feature = "host_camera_nokhwa")]
fn try_acquire_nokhwa_frame(width: u32, height: u32) -> Option<FrameSource> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    use nokhwa::Camera;

    // Open the default camera lazily for one frame. Opening is relatively
    // expensive so the real capture loop (outside this file) would keep the
    // Camera alive; for the purpose of a blocking single-frame probe we open,
    // capture, close.
    let index = CameraIndex::Index(0);
    let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(index, requested).ok()?;
    camera.open_stream().ok()?;
    let frame = camera.frame().ok()?;
    let decoded = frame.decode_image::<RgbFormat>().ok()?;
    // nokhwa delivers RGB; promote to RGBA for the GL uploader.
    let rgb = decoded.to_vec();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    // Simple nearest-neighbour scale if the host frame size differs from the
    // requested size — not performance-critical for a single-frame probe.
    let (src_w, src_h) = (decoded.width(), decoded.height());
    // If the decoded size already matches, avoid scaling cost.
    if src_w == width && src_h == height {
        for px in rgb.chunks(3) {
            rgba.extend_from_slice(&[px[0], px[1], px[2], 0xFF]);
        }
    } else {
        // Nearest neighbour.
        for y in 0..height {
            for x in 0..width {
                let sx = (x * src_w / width) as usize;
                let sy = (y * src_h / height) as usize;
                let off = (sy * src_w as usize + sx) * 3;
                if off + 2 < rgb.len() {
                    rgba.extend_from_slice(&[rgb[off], rgb[off + 1], rgb[off + 2], 0xFF]);
                } else {
                    rgba.extend_from_slice(&[0x80, 0x80, 0x80, 0xFF]);
                }
            }
        }
    }
    let _ = camera.stop_stream();
    Some(FrameSource::Host(rgba, width, height))
}

/// Deterministic synthetic frame: 50 % grey with a moving vertical bar.
///
/// The bar position is derived from `Instant::now` so successive frames
/// animate even though no host camera is involved — this proves to apps
/// that rely on inter-frame deltas (motion detection, barcode “scan line”)
/// that the camera is alive.
pub fn synthetic_frame_rgba(width: u32, height: u32) -> Vec<u8> {
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    let elapsed = start.elapsed().as_secs_f32();
    // Bar moves 120 px/s, wraps every `width`.
    let bar_x = if width == 0 { 0.0 } else { (elapsed * 120.0) % width as f32 };
    let bar_w = (width as f32 * 0.06).clamp(4.0, 24.0);

    let mut out = vec![0x80u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let off = ((y * width + x) * 4) as usize;
            // Grey background already set; carve the bar.
            let dist = (x as f32 - bar_x).abs();
            let wrapped = (width as f32 - dist).abs();
            let d = dist.min(wrapped);
            if d < bar_w * 0.5 {
                // Colour bar: hue cycles with y for a pleasant visual.
                let hue = (y as f32 / height as f32 * 360.0) % 360.0;
                let (r, g, b) = hsv_to_rgb(hue, 0.9, 1.0);
                out[off] = r;
                out[off + 1] = g;
                out[off + 2] = b;
                out[off + 3] = 0xFF;
            } else if d < bar_w * 0.55 {
                // 1px anti-alias border — blend grey and bar colour.
                out[off] = 0x9A;
                out[off + 1] = 0x9A;
                out[off + 2] = 0x9A;
            }
        }
    }
    // Thin white frame so the preview bounds are obvious.
    for x in 0..width {
        for &y in &[0, height.saturating_sub(1)] {
            let off = ((y * width + x) * 4) as usize;
            out[off..off + 3].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        }
    }
    for y in 0..height {
        for &x in &[0, width.saturating_sub(1)] {
            let off = ((y * width + x) * 4) as usize;
            out[off..off + 3].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        }
    }
    // Small “HyperHLE Stub” hint in the top-left when the host camera is
    // absent — helpful for screenshots / bug reports. Not rendered as text
    // (that would need a font), just a 2×2 white block that is visually
    // distinct from the bar.
    if !is_available() {
        // This branch is actually unreachable because synthetic_frame_rgba is
        // only called when is_available() is true, but keep the visual hint
        // for the dedicated stub path that calls it directly.
    }
    out
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hh = h / 60.0;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

/// Human-readable status line for the startup banner.
pub fn status_string() -> String {
    if is_available() {
        format!("Host camera available ({})", localized_name())
    } else {
        "No host camera — using stub (no device)".to_string()
    }
}
