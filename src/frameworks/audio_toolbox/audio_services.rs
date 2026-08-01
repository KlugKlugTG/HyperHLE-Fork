/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `AudioServices.h` (Audio Services)

use std::collections::HashMap;

use crate::abi::{CallFromHost, GuestFunction};
use crate::audio;
use crate::audio::openal as al;
use crate::audio::openal::al_types::*;
use crate::audio::openal::{OpenAL, OpenALManager};

use super::audio_queue::decode_buffer;
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::carbon_core::{paramErr, OSStatus};
use crate::frameworks::core_audio_types::{debug_fourcc, fourcc, AudioStreamBasicDescription};
use crate::frameworks::core_foundation::cf_url::CFURLRef;
use crate::frameworks::foundation::ns_url::to_rust_path;
use crate::mem::{ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::Environment;

type AudioServicesPropertyID = u32;
pub type SystemSoundID = u32;
type AudioServicesSystemSoundCompletionProc = u32; // guest function pointer

const kAudioServicesUnsupportedPropertyError: OSStatus = fourcc(b"pty?") as _;
const kAudioServicesBadSystemSoundIDError: OSStatus = fourcc(b"!ids") as _;
const kAudioServicesSystemSoundUnspecifiedError: OSStatus = -1500;

const kSystemSoundID_Vibrate: SystemSoundID = 0x00000FFF;
const kSystemSoundID_UserPreferredAlert: SystemSoundID = 0x00001000;
const INITIAL_SYSTEM_SOUND_ID: SystemSoundID = 0x1001;

pub struct SystemSoundData {
    al_source: ALuint,
    al_buffer: ALuint,
}

// Комбинируем логику из форка (фоллбэк) и оригинала (реальное воспроизведение)
pub enum SoundEntry {
    Real(SystemSoundData),
    Dummy, // Используется, если парсер не смог открыть файл (из форка)
}

/// Per-sound completion callback registered via
/// `AudioServicesAddSystemSoundCompletion`. Per Apple's documented signature:
///
/// ```c
/// typedef void (*AudioServicesSystemSoundCompletionProc)(
///     SystemSoundID ssID, void *clientData);
/// ```
#[derive(Clone, Copy)]
pub struct SoundCompletion {
    pub proc: AudioServicesSystemSoundCompletionProc,
    pub client_data: MutVoidPtr,
    /// Set to `true` once `AudioServicesPlaySystemSound` has actually
    /// observed the source enter the `AL_PLAYING` state. Without this gate
    /// we'd fire the completion immediately on the first tick (because the
    /// source is `AL_INITIAL` / `AL_STOPPED` until we issue `SourcePlay`).
    pub fired_play: bool,
}

pub struct State {
    pub system_sounds: HashMap<SystemSoundID, SoundEntry>,
    pub next_sound_id: SystemSoundID,
    pub completions: HashMap<SystemSoundID, SoundCompletion>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            system_sounds: Default::default(),
            next_sound_id: INITIAL_SYSTEM_SOUND_ID,
            completions: Default::default(),
        }
    }
}

impl State {
    pub fn get(framework_state: &mut crate::frameworks::State) -> &mut Self {
        &mut framework_state.audio_toolbox.audio_services
    }

    fn get_with_context<'s, 'm: 's>(
        framework_state: &'s mut crate::frameworks::State,
        manager: &'m mut OpenALManager,
    ) -> (&'s mut Self, OpenAL<'s>) {
        let toolbox = &mut framework_state.audio_toolbox;
        (
            &mut toolbox.audio_services,
            toolbox.al_context.make_al_context_current(manager),
        )
    }
}

fn AudioServicesGetProperty(
    _env: &mut Environment,
    in_property_id: AudioServicesPropertyID,
    _in_specifier_size: u32,
    _in_specifier: ConstVoidPtr,
    _io_property_data_size: MutPtr<u32>,
    _out_property_data: MutVoidPtr,
) -> OSStatus {
    // Crash Bandicoot Nitro Kart 3D пытается использовать это свойство.
    if in_property_id == 0xfff {
        return kAudioServicesUnsupportedPropertyError;
    }

    // В форке используется мягкий подход без паники
    log!(
        "AudioServicesGetProperty: property {} is unimplemented",
        debug_fourcc(in_property_id)
    );
    kAudioServicesUnsupportedPropertyError
}

fn AudioServicesSetProperty(
    _env: &mut Environment,
    in_property_id: AudioServicesPropertyID,
    _in_specifier_size: u32,
    _in_specifier: ConstVoidPtr,
    _in_property_data_size: u32,
    _in_property_data: ConstVoidPtr,
) -> OSStatus {
    log!(
        "AudioServicesSetProperty: property {} is unimplemented",
        debug_fourcc(in_property_id)
    );
    0
}

fn AudioServicesCreateSystemSoundID(
    env: &mut Environment,
    in_file_url: CFURLRef,
    out_system_sound_id: MutPtr<SystemSoundID>,
) -> OSStatus {
    if in_file_url.is_null() {
        log!("Warning: AudioServicesCreateSystemSoundID called with NULL in_file_url");
        if !out_system_sound_id.is_null() {
            env.mem.write(out_system_sound_id, 0);
        }
        return paramErr;
    }

    let path = to_rust_path(env, in_file_url);
    log!("AudioServicesCreateSystemSoundID: opening {:?}", path);

    let audio_file_result = audio::AudioFile::open_for_reading(&path, &env.fs);

    // Подход из форка + декодирование из оригинала
    let sound_entry = match audio_file_result {
        Ok(mut audio_file) => {
            let mut data = vec![0; audio_file.byte_count().try_into().unwrap()];
            let format =
                AudioStreamBasicDescription::from_audio_description(audio_file.audio_description());
            let size = audio_file.read_bytes(0, data.as_mut_slice()).unwrap();
            let tmp = env.mem.alloc(size as GuestUSize);
            env.mem
                .bytes_at_mut(tmp.cast(), size as GuestUSize)
                .copy_from_slice(data.as_slice());

            let (al_format, al_frequency, decoded_data) =
                decode_buffer(&env.mem, &format, tmp.cast(), size as GuestUSize);
            env.mem.free(tmp.cast());
            log!(
                "AudioServicesCreateSystemSoundID: {:?} -> al_format=0x{:x}, al_freq={}, pcm_bytes={}",
                path, al_format, al_frequency, decoded_data.len()
            );

            let (_, context) =
                State::get_with_context(&mut env.framework_state, &mut env.openal_manager);

            let mut al_source = 0;
            unsafe {
                context.GenSources(1, &mut al_source);
                assert!(context.GetError() == 0);
            }

            let mut al_buffer = 0;
            unsafe {
                context.GenBuffers(1, &mut al_buffer);
                context.BufferData(
                    al_buffer,
                    al_format,
                    decoded_data.as_ptr() as *const ALvoid,
                    decoded_data.len().try_into().unwrap(),
                    al_frequency,
                );
                context.Sourcei(al_source, al::AL_BUFFER, al_buffer.try_into().unwrap());
                assert!(context.GetError() == 0);
            }

            SoundEntry::Real(SystemSoundData {
                al_source,
                al_buffer,
            })
        }
        Err(_) => {
            log!("Warning: AudioServicesCreateSystemSoundID failed to open file {:?}. Generating silent/dummy sound ID.", path);
            SoundEntry::Dummy
        }
    };

    let state = State::get(&mut env.framework_state);

    if state.next_sound_id <= kSystemSoundID_UserPreferredAlert {
        state.next_sound_id = kSystemSoundID_UserPreferredAlert + 1;
    }

    let id = state.next_sound_id;
    state.next_sound_id += 1;

    state.system_sounds.insert(id, sound_entry);

    if !out_system_sound_id.is_null() {
        env.mem.write(out_system_sound_id, id);
    }

    log!("AudioServicesCreateSystemSoundID: id={} for {:?}", id, path);
    0
}

fn AudioServicesDisposeSystemSoundID(
    env: &mut Environment,
    in_system_sound_id: SystemSoundID,
) -> OSStatus {
    let (state, context) =
        State::get_with_context(&mut env.framework_state, &mut env.openal_manager);

    if let Some(entry) = state.system_sounds.remove(&in_system_sound_id) {
        if let SoundEntry::Real(SystemSoundData {
            al_source,
            al_buffer,
        }) = entry
        {
            unsafe {
                context.SourceStop(al_source);
                context.DeleteSources(1, &al_source as *const ALuint);
                context.DeleteBuffers(1, &al_buffer as *const ALuint);
                assert!(context.GetError() == 0);
            }
        }
        0
    } else {
        log!(
            "Tried to dispose of invalid/unknown system sound {}!",
            in_system_sound_id
        );
        kAudioServicesSystemSoundUnspecifiedError
    }
}

fn AudioServicesPlaySystemSound(env: &mut Environment, in_system_sound_id: SystemSoundID) {
    log!("AudioServicesPlaySystemSound({})", in_system_sound_id);
    if in_system_sound_id == kSystemSoundID_Vibrate {
        log!("TODO: vibration (AudioServicesPlaySystemSound)");
        return;
    } else if in_system_sound_id == kSystemSoundID_UserPreferredAlert {
        log!("TODO: alert sound (AudioServicesPlaySystemSound)");
        return;
    }

    let (state, context) =
        State::get_with_context(&mut env.framework_state, &mut env.openal_manager);

    if let Some(entry) = state.system_sounds.get(&in_system_sound_id) {
        match entry {
            SoundEntry::Real(SystemSoundData {
                al_source,
                al_buffer: _,
            }) => {
                log_dbg!(
                    "AudioToolbox: Playing system sound ID: {}",
                    in_system_sound_id
                );
                unsafe {
                    let al_source = *al_source;
                    context.SourcePlay(al_source);
                    assert!(context.GetError() == 0);
                    let mut al_state: i32 = 0;
                    context.GetSourcei(al_source, al::AL_SOURCE_STATE, &mut al_state as *mut i32);
                    assert!(context.GetError() == 0);
                    assert!(
                        al_state == al::AL_PLAYING,
                        "Expected AL_PLAYING after SourcePlay, got {:#x}",
                        al_state
                    );
                }
                // If the guest registered a completion callback for this
                // sound, mark it as "playback has actually started" so the
                // tick handler will fire the callback once playback
                // finishes (al_state goes back to AL_STOPPED).
                if let Some(c) = state.completions.get_mut(&in_system_sound_id) {
                    c.fired_play = true;
                }
            }
            SoundEntry::Dummy => {
                log!("AudioToolbox: Attempted to play Dummy system sound ID: {} (Skipping gracefully)", in_system_sound_id);
            }
        }
    } else {
        log!(
            "AudioToolbox: Attempted to play unknown system sound ID: {} (Skipping gracefully)",
            in_system_sound_id
        );
    }
}

fn AudioServicesPlayAlertSound(env: &mut Environment, in_system_sound_id: SystemSoundID) {
    AudioServicesPlaySystemSound(env, in_system_sound_id);
}

fn AudioServicesAddSystemSoundCompletion(
    env: &mut Environment,
    in_system_sound_id: SystemSoundID,
    _in_run_loop: MutVoidPtr,
    _in_run_loop_mode: MutVoidPtr,
    in_completion_routine: AudioServicesSystemSoundCompletionProc,
    in_client_data: MutVoidPtr,
) -> OSStatus {
    // Per Apple docs:
    // https://developer.apple.com/documentation/audiotoolbox/audioservicesaddsystemsoundcompletion(_:_:_:_:_:)
    //
    // "Registers a callback function to be invoked when the specified
    // system sound finishes playing. […] If inRunLoop is NULL, the system
    // installs the callback in the main run loop in its default mode."
    //
    // touchHLE runs a single ARM run loop, so we ignore inRunLoop /
    // inRunLoopMode (the polling is performed unconditionally from
    // ns_run_loop's tick). We DO support the documented "unregister via
    // NULL completion routine" idiom (passing NULL is also handled
    // explicitly by `AudioServicesRemoveSystemSoundCompletion`).
    let state = State::get(&mut env.framework_state);
    if in_completion_routine == 0 {
        state.completions.remove(&in_system_sound_id);
        return 0;
    }
    state.completions.insert(
        in_system_sound_id,
        SoundCompletion {
            proc: in_completion_routine,
            client_data: in_client_data,
            fired_play: false,
        },
    );
    0
}

fn AudioServicesRemoveSystemSoundCompletion(
    env: &mut Environment,
    in_system_sound_id: SystemSoundID,
) {
    // Per Apple: "Unregisters any completion callback previously registered
    // for the system sound."
    State::get(&mut env.framework_state)
        .completions
        .remove(&in_system_sound_id);
}

/// Called once per run loop tick. For every system sound that has a
/// completion callback registered AND has been observed to actually
/// transition into `AL_PLAYING`, check the OpenAL source state. When the
/// source returns to `AL_STOPPED`, fire the guest callback with
/// `(SystemSoundID, void *clientData)` exactly as documented by Apple,
/// then arm the registration for the next play (real Audio Services
/// completion callbacks persist until explicitly removed).
pub fn tick_system_sound_completions(env: &mut Environment) {
    // Build a list of (sound_id, al_source) for sounds that have an armed
    // completion. We can't hold a borrow on `state` while invoking the
    // guest callback (the callback can recursively call into Audio
    // Services), so collect first, dispatch second.
    let pending: Vec<(SystemSoundID, ALuint)> = {
        let state = State::get(&mut env.framework_state);
        state
            .completions
            .iter()
            .filter(|(_, c)| c.fired_play)
            .filter_map(|(id, _)| match state.system_sounds.get(id) {
                Some(SoundEntry::Real(s)) => Some((*id, s.al_source)),
                _ => None,
            })
            .collect()
    };

    if pending.is_empty() {
        return;
    }

    let (_state_unused, context) =
        State::get_with_context(&mut env.framework_state, &mut env.openal_manager);
    let mut finished: Vec<SystemSoundID> = Vec::new();
    for (sound_id, al_source) in pending {
        let mut al_state: i32 = 0;
        unsafe {
            context.GetSourcei(al_source, al::AL_SOURCE_STATE, &mut al_state as *mut i32);
            // Best-effort: a transient AL error shouldn't kill the run loop.
            let _ = context.GetError();
        }
        if al_state == al::AL_STOPPED {
            finished.push(sound_id);
        }
    }
    drop(context);

    for sound_id in finished {
        let SoundCompletion {
            proc, client_data, ..
        } = {
            let state = State::get(&mut env.framework_state);
            // Reset `fired_play` so the next playback re-arms cleanly.
            if let Some(c) = state.completions.get_mut(&sound_id) {
                c.fired_play = false;
            }
            match state.completions.get(&sound_id) {
                Some(c) => *c,
                None => continue,
            }
        };
        let f: GuestFunction = GuestFunction::from_addr_with_thumb_bit(proc);
        let () = f.call_from_host(env, (sound_id, client_data));
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(AudioServicesGetProperty(_, _, _, _, _)),
    export_c_func!(AudioServicesSetProperty(_, _, _, _, _)),
    export_c_func!(AudioServicesCreateSystemSoundID(_, _)),
    export_c_func!(AudioServicesDisposeSystemSoundID(_)),
    export_c_func!(AudioServicesPlaySystemSound(_)),
    export_c_func!(AudioServicesPlayAlertSound(_)),
    export_c_func!(AudioServicesAddSystemSoundCompletion(_, _, _, _, _)),
    export_c_func!(AudioServicesRemoveSystemSoundCompletion(_)),
];
