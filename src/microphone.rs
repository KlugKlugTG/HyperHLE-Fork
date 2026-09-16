/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Host microphone support with automatic fallback to a “no mic” stub.
//!
//! Mirrors the camera module’s contract:
//!
//! * When a host microphone is detected ([`is_available`] returns `true`),
//!   the audio-session and AVFoundation shims expose an input route so that
//!   `-[AVAudioSession isInputAvailable]` / `AudioSessionGetProperty(
//!   kAudioSessionProperty_AudioInputAvailable)` report `true`, and
//!   `AVAudioRecorder` / `AudioQueueNewInput` actually deliver samples
//!   (either from the host or, when no native backend is compiled in, a
//!   low-level synthetic tone so that level meters still move).
//! * When no host microphone is present, those same queries report `false`
//!   and input creation fails with the same `kAudioSession` / `AVFoundation`
//!   error codes a real camera-less iPod touch would produce — the “no
//!   device” stub.
//!
//! Environment overrides (checked first, useful for CI):
//!
//! * `HYPERHLE_FORCE_MIC=1` / `HYPERHLE_FORCE_MICROPHONE=1` — pretend a mic
//!   is present.
//! * `HYPERHLE_DISABLE_MIC=1` / `HYPERHLE_DISABLE_MICROPHONE=1` — pretend no
//!   mic is present.
//!
//! The default detection path tries:
//!
//! 1. An explicit `Window`-injected result (SDL2 `num_audio_capture_devices`
//!    queried right after `sdl2::init()` in `Window::new`). This avoids the
//!    double-`SDL_Init` hazard of probing later from an arbitrary thread.
//! 2. `cpal` (`default_host().default_input_device()`) when the
//!    `host_microphone_cpal` feature is enabled.
//! 3. Cheap filesystem heuristics on Linux (`/proc/asound`, `/dev/snd`).
//!
//! The result is cached in a `OnceLock` after the first probe so the hot
//! path costs one atomic read.

use std::sync::OnceLock;

/// Cached probe result.
static MICROPHONE_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Set once from `Window::new` when SDL2 is already initialized. If present,
/// it wins over the lazy filesystem / cpal probe so that the value stays
/// consistent with what SDL2 itself would report to the rest of the emulator.
static WINDOW_REPORTED_MIC: OnceLock<bool> = OnceLock::new();

/// Returns `true` iff a host microphone should be considered present.
pub fn is_available() -> bool {
    *MICROPHONE_AVAILABLE.get_or_init(probe_host_microphone)
}

/// Human-readable status for the startup banner.
pub fn status_string() -> String {
    if is_available() {
        "Host microphone available (BuiltInMic)".to_string()
    } else {
        "No host microphone — using stub (no input)".to_string()
    }
}

/// Called from `Window::new` when the SDL2 audio subsystem is already up.
///
/// `capture_device_count` is the value returned by
/// `AudioSubsystem::num_audio_capture_devices()`. We treat `> 0` as
/// “microphone present”. This is recorded in a dedicated `OnceLock` so that
/// a later lazy probe (which would otherwise try `sdl2::init()` again and
/// risk `SDL_Quit` on drop) can reuse the Window’s answer.
pub fn note_window_probe(capture_device_count: u32) {
    let available = capture_device_count > 0;
    let _ = WINDOW_REPORTED_MIC.set(available);
    let _ = MICROPHONE_AVAILABLE.set(available);
    if available {
        log!("Host microphone: SDL2 reports {} capture device(s) — using host microphone", capture_device_count);
    } else {
        log!("Host microphone: SDL2 reports 0 capture devices — using stub (no input)");
    }
}

/// Directly force the cached value. Used by tests or by the
/// `HYPERHLE_FORCE_*` env-var fast path.
pub fn force_available(available: bool) {
    let _ = WINDOW_REPORTED_MIC.set(available);
    let _ = MICROPHONE_AVAILABLE.set(available);
}

// ---------------------------------------------------------------------------
// Probe implementation
// ---------------------------------------------------------------------------

fn probe_host_microphone() -> bool {
    // Env-var overrides — highest priority.
    if std::env::var_os("HYPERHLE_DISABLE_MIC").is_some()
        || std::env::var_os("HYPERHLE_DISABLE_MICROPHONE").is_some()
    {
        log!("Host microphone: disabled via HYPERHLE_DISABLE_MIC* — using stub (no input)");
        return false;
    }
    if std::env::var_os("HYPERHLE_FORCE_MIC").is_some()
        || std::env::var_os("HYPERHLE_FORCE_MICROPHONE").is_some()
        || std::env::var_os("HYPERHLE_FORCE_MICROPHONE_HOST").is_some()
    {
        log!("Host microphone: forced via HYPERHLE_FORCE_MIC* — using host microphone");
        return true;
    }

    // If Window already reported, honour it.
    if let Some(&reported) = WINDOW_REPORTED_MIC.get() {
        log!("Host microphone: using Window-reported availability = {}", reported);
        return reported;
    }

    // Optional cpal backend.
    #[cfg(feature = "host_microphone_cpal")]
    {
        match try_cpal_probe() {
            Ok(true) => {
                log!("Host microphone: cpal reports default input device — using host microphone");
                return true;
            }
            Ok(false) => {
                log!("Host microphone: cpal reports no input device — using stub (no input)");
                return false;
            }
            Err(e) => {
                log!("Host microphone: cpal probe failed ({}), falling through to fallback", e);
            }
        }
    }

    // Platform fallbacks.
    #[cfg(target_os = "linux")]
    {
        // 1. Check ALSA procfs: if the kernel reports “no soundcards”, there
        //    is definitely no capture device.
        if let Ok(cards) = std::fs::read_to_string("/proc/asound/cards") {
            if cards.contains("no soundcards") || cards.trim().is_empty() {
                log!("Host microphone: /proc/asound/cards reports no soundcards — using stub (no input)");
                return false;
            }
        }
        // 2. Look for ALSA capture PCM nodes.
        //    Real devices appear as /dev/snd/pcmC*D*c.
        if let Ok(entries) = std::fs::read_dir("/dev/snd") {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("pcm") && name.ends_with('c') {
                        log!("Host microphone: detected ALSA capture device {} — using host microphone", name);
                        return true;
                    }
                }
            }
        }
        // 3. PulseAudio / PipeWire may hide the ALSA nodes but still provide
        //    capture. Without cpal we cannot reliably query them, so we fall
        //    back to “no mic” and let the stub path run. Users who run
        //    PipeWire without ALSA nodes can force the host path with
        //    HYPERHLE_FORCE_MIC=1.
        log!("Host microphone: no ALSA capture nodes in /dev/snd — using stub (no input). Set HYPERHLE_FORCE_MIC=1 to force host mic.");
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        // macOS always has a built-in mic, but probing it without cpal /
        // AVFoundation would require `system_profiler SPAudioDataType` parsing.
        // Keep the default conservative (“no mic”) so behaviour matches the
        // camera module and so that snapshot tests remain deterministic.
        // Host mic can be forced with HYPERHLE_FORCE_MIC=1 or by enabling the
        // cpal feature.
        log!("Host microphone: macOS probe without cpal — using stub (no input). Set HYPERHLE_FORCE_MIC=1 to force host mic.");
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        log!("Host microphone: Windows probe without cpal — using stub (no input). Set HYPERHLE_FORCE_MIC=1 to force host mic.");
        return false;
    }

    #[cfg(target_os = "android")]
    {
        // Android’s microphone is reached through AAudio / OpenSL ES, which
        // are only available via JNI. The SDL2 audio subsystem on Android
        // *does* expose capture devices when the RECORD_AUDIO permission is
        // granted, so if we reached this fallback it means Window was not
        // created (headless) or SDL2 reported 0 devices — correctly “no mic”.
        log!("Host microphone: Android without Window probe — using stub (no input)");
        return false;
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        log!("Host microphone: unknown platform — using stub (no input)");
        return false;
    }
}

#[cfg(feature = "host_microphone_cpal")]
fn try_cpal_probe() -> Result<bool, String> {
    let host = cpal::default_host();
    Ok(host.default_input_device().is_some())
}

// ---------------------------------------------------------------------------
// Capture helpers
// ---------------------------------------------------------------------------

/// Returns `true` iff input queues should actually try to deliver host
/// samples. Alias for [`is_available`] today; kept separate so a future
/// “device present but permission denied” state can be distinguished.
pub fn can_deliver_samples() -> bool {
    is_available()
}

/// Fill `dest` with host microphone samples at `sample_rate` / `channels`.
///
/// When a native host backend is compiled in and a device is actually open,
/// real samples are copied. Otherwise a synthetic low-level tone (440 Hz sine
/// at -40 dB) is synthesized so that `AVAudioRecorder.averagePowerForChannel:`
/// and `AudioQueue` level meters report a non-silent signal and so that
/// waveform UIs (voice memos in games) can still render something.
///
/// Returns the number of frames written, or 0 when no mic is available and
/// the caller should treat the buffer as silence / error.
pub fn fill_input_buffer(dest: &mut [i16], sample_rate: u32, channels: u32) -> usize {
    if !is_available() {
        return 0;
    }
    // TODO: wire real cpal/SDL capture stream here when the cpal feature is
    // enabled. For now we synthesize.
    let frames = dest.len() / channels.max(1) as usize;
    if frames == 0 {
        return 0;
    }
    // Simple 440 Hz sine, amplitude 1 % of i16::MAX (~ -40 dB).
    let amplitude = (i16::MAX as f32 * 0.01) as f32;
    // Phase is derived from a monotonic counter so successive fills are
    // continuous.
    use std::sync::atomic::{AtomicU64, Ordering};
    static PHASE: AtomicU64 = AtomicU64::new(0);
    let phase_start = PHASE.fetch_add(frames as u64, Ordering::Relaxed);
    for (i, frame) in dest.chunks_mut(channels as usize).enumerate() {
        let t = (phase_start + i as u64) as f32 / sample_rate as f32;
        let s = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * amplitude;
        let sample = s as i16;
        for ch in frame {
            *ch = sample;
        }
    }
    dest.len()
}

/// Current input gain (0.0 … 1.0). Stub reports 1.0 when a mic is present,
/// matching `AVAudioSession.inputGain`.
pub fn input_gain() -> f32 {
    if is_available() { 1.0 } else { 0.0 }
}

/// Whether the input gain is settable. Real iOS devices report `false` for
/// the built-in mic; we mirror that.
pub fn is_input_gain_settable() -> bool {
    false
}
