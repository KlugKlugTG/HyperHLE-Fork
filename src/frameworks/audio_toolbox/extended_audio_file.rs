/*
 * Эта лицензия Source Code Form подпадает под условия Mozilla Public
 * License, v. 2.0. Если копия MPL не распространялась вместе с этим
 * файлом, вы можете получить ее на https://mozilla.org/MPL/2.0/.
 */
//! `ExtendedAudioFile.h` (Extended Audio File Services)
//!
//! Реализовано как обертка над Audio File Services, работающая с форком.

// TODO: Конвертация аудио форматов

use super::audio_file::{
    kAudioFileBadPropertySizeError, kAudioFilePropertyDataFormat, kAudioFileReadPermission,
    property_size, AudioFileClose, AudioFileGetProperty, AudioFileHostObject, AudioFileID,
    AudioFileOpenURL, AudioFileReadBytes,
};
use super::audio_queue::is_supported_audio_format;
use super::audio_unit::AudioBufferList;
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::carbon_core::{eofErr, OSStatus};
use crate::frameworks::core_audio_types::{
    debug_fourcc, fourcc, kAudioFormatLinearPCM, AudioStreamBasicDescription,
};
use crate::frameworks::core_foundation::cf_url::CFURLRef;
use crate::frameworks::foundation::ns_url::to_rust_path;
use crate::mem::{guest_size_of, ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr, SafeRead};
use crate::Environment;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    pub extended_audio_files: HashMap<ExtAudioFileRef, ExtAudioFileHostObject>,
}
impl State {
    pub fn get(framework_state: &mut crate::frameworks::State) -> &mut Self {
        &mut framework_state.audio_toolbox.ext_audio_file
    }
}

pub struct ExtAudioFileHostObject {
    guest_audio_file: AudioFileID,
    client_data_format: Option<AudioStreamBasicDescription>,
    current_bytes_read: i64,
}

#[repr(C, packed)]
pub struct OpaqueExtAudioFile {
    _filler: u8,
}
unsafe impl SafeRead for OpaqueExtAudioFile {}

type ExtAudioFileRef = MutPtr<OpaqueExtAudioFile>;

/// Обычно FourCC.
type ExtAudioFilePropertyID = u32;
const kExtAudioFileProperty_FileDataFormat: ExtAudioFilePropertyID = fourcc(b"ffmt");
const kExtAudioFileProperty_ClientDataFormat: ExtAudioFilePropertyID = fourcc(b"cfmt");

fn ExtAudioFileOpenURL(
    env: &mut Environment,
    in_url: CFURLRef,
    out_ext_audio_file: MutPtr<ExtAudioFileRef>,
) -> OSStatus {
    return_if_null!(in_url);

    let path = to_rust_path(env, in_url);
    log_dbg!(
        "ExtAudioFileOpenURL({:?} '{:?}', {:?})",
        in_url,
        path,
        out_ext_audio_file
    );

    let audio_file_ptr: MutPtr<AudioFileID> = env.mem.alloc(guest_size_of::<AudioFileID>()).cast();
    // Используем Audio File Services под капотом
    let res = AudioFileOpenURL(env, in_url, kAudioFileReadPermission, 0, audio_file_ptr);
    let guest_audio_file = env.mem.read(audio_file_ptr);
    env.mem.free(audio_file_ptr.cast());
    if res != 0 {
        log!(
            "Ошибка: ExtAudioFileOpenURL({:?} '{:?}', {:?}) завершилась с кодом: {:?}",
            in_url,
            path,
            out_ext_audio_file,
            res
        );
        return res;
    }

    let host_object = ExtAudioFileHostObject {
        guest_audio_file,
        client_data_format: None,
        current_bytes_read: 0,
    };

    let guest_extended_audio_file = env.mem.alloc_and_write(OpaqueExtAudioFile { _filler: 0 });
    State::get(&mut env.framework_state)
        .extended_audio_files
        .insert(guest_extended_audio_file, host_object);

    env.mem.write(out_ext_audio_file, guest_extended_audio_file);

    0 // успех
}

fn ExtAudioFileGetProperty(
    env: &mut Environment,
    in_ext_audio_file: ExtAudioFileRef,
    in_property_id: ExtAudioFilePropertyID,
    io_property_data_size: MutPtr<u32>,
    out_property_data: MutVoidPtr,
) -> OSStatus {
    return_if_null!(in_ext_audio_file);

    let audio_file_property_id = match in_property_id {
        kExtAudioFileProperty_FileDataFormat => kAudioFilePropertyDataFormat,
        _ => {
            log!(
                "Warning: ExtAudioFileGetProperty: unsupported property {}; \
                 returning kAudioFileBadPropertySizeError.",
                debug_fourcc(in_property_id)
            );
            return kAudioFileBadPropertySizeError;
        }
    };

    let required_size = property_size(audio_file_property_id);
    if env.mem.read(io_property_data_size) != required_size {
        log!("Внимание: ExtAudioFileGetProperty() завершилась с ошибкой размера");
        return kAudioFileBadPropertySizeError;
    }

    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get(&in_ext_audio_file)
    else {
        log!(
            "Warning: ExtAudioFileGetProperty({:?}): unknown / disposed handle.",
            in_ext_audio_file
        );
        return kAudioFileBadPropertySizeError;
    };

    // Делегируем вызов в обычный AudioFile
    AudioFileGetProperty(
        env,
        host_object.guest_audio_file,
        audio_file_property_id,
        io_property_data_size,
        out_property_data,
    )
}

fn ExtAudioFileSetProperty(
    env: &mut Environment,
    in_ext_audio_file: ExtAudioFileRef,
    in_property_id: ExtAudioFilePropertyID,
    in_property_data_size: u32,
    in_property_data: ConstVoidPtr,
) -> OSStatus {
    return_if_null!(in_ext_audio_file);

    if in_property_id != kExtAudioFileProperty_ClientDataFormat {
        log!(
            "Warning: ExtAudioFileSetProperty: unsupported property {}; \
             returning kAudioFileBadPropertySizeError.",
            debug_fourcc(in_property_id)
        );
        return kAudioFileBadPropertySizeError;
    }
    if in_property_data_size != guest_size_of::<AudioStreamBasicDescription>() {
        log!(
            "Warning: ExtAudioFileSetProperty(ClientDataFormat): wrong size {}.",
            in_property_data_size
        );
        return kAudioFileBadPropertySizeError;
    }

    let audio_desc_ptr: ConstPtr<AudioStreamBasicDescription> = in_property_data.cast();
    let client_audio_desc = env.mem.read(audio_desc_ptr);
    log_dbg!("ExtAudioFileSetProperty {:?}", client_audio_desc);
    let format_id = client_audio_desc.format_id;
    if format_id != kAudioFormatLinearPCM {
        log!(
            "Warning: ExtAudioFileSetProperty(ClientDataFormat): only PCM is \
             supported, got {}.",
            debug_fourcc(format_id)
        );
        return kAudioFileBadPropertySizeError;
    }
    if !is_supported_audio_format(&client_audio_desc) {
        log!(
            "Warning: ExtAudioFileSetProperty(ClientDataFormat): unsupported \
             format {:?}.",
            client_audio_desc
        );
        return kAudioFileBadPropertySizeError;
    }

    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get_mut(&in_ext_audio_file)
    else {
        log!(
            "Warning: ExtAudioFileSetProperty({:?}): unknown / disposed handle.",
            in_ext_audio_file
        );
        return kAudioFileBadPropertySizeError;
    };
    if host_object.client_data_format.is_some() {
        log!(
            "Warning: ExtAudioFileSetProperty({:?}): ClientDataFormat already \
             set; overwriting.",
            in_ext_audio_file
        );
    }
    host_object.client_data_format = Some(client_audio_desc);

    // Достаем объект AudioFile, чтобы проверить описание.
    // Обрабатываем перечисление из форка (Real или Dummy).
    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get(&in_ext_audio_file)
    else {
        return kAudioFileBadPropertySizeError;
    };
    let Some(other_host_object) = env
        .framework_state
        .audio_toolbox
        .audio_file
        .audio_files
        .get(&host_object.guest_audio_file)
    else {
        log!(
            "Warning: ExtAudioFileSetProperty({:?}): underlying AudioFile is \
             gone; accepting client format but reads will fail.",
            in_ext_audio_file
        );
        return 0;
    };

    let _audio_desc = match other_host_object {
        AudioFileHostObject::Real(file) => {
            AudioStreamBasicDescription::from_audio_description(file.audio_description())
        }
        AudioFileHostObject::Dummy { format, .. } => *format,
    };

    // TODO: Поддержка конвертации аудио форматов
    // assert_eq!(_audio_desc, client_audio_desc);

    0 // успех
}

fn ExtAudioFileRead(
    env: &mut Environment,
    in_ext_audio_file: ExtAudioFileRef,
    io_number_frames: MutPtr<u32>,
    io_data: MutPtr<AudioBufferList<1>>,
) -> OSStatus {
    return_if_null!(in_ext_audio_file);

    let mut audio_buffer_list = env.mem.read(io_data);
    let num_buffers = audio_buffer_list.number_buffers;
    if num_buffers != 1 {
        log!(
            "Warning: ExtAudioFileRead: only 1 buffer is supported, got {}.",
            num_buffers
        );
        env.mem.write(io_number_frames, 0);
        return kAudioFileBadPropertySizeError;
    }

    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get(&in_ext_audio_file)
    else {
        log!(
            "Warning: ExtAudioFileRead({:?}): unknown / disposed handle.",
            in_ext_audio_file
        );
        env.mem.write(io_number_frames, 0);
        return kAudioFileBadPropertySizeError;
    };

    let Some(client_data_format) = host_object.client_data_format else {
        log!(
            "Warning: ExtAudioFileRead({:?}) before ClientDataFormat was set; \
             returning 0 frames.",
            in_ext_audio_file
        );
        env.mem.write(io_number_frames, 0);
        return kAudioFileBadPropertySizeError;
    };

    audio_buffer_list.buffers[0].number_channels = client_data_format.channels_per_frame;

    let number_frames = env.mem.read(io_number_frames);
    let bytes_per_frame = client_data_format.bytes_per_frame;
    if bytes_per_frame == 0 {
        log!(
            "Warning: ExtAudioFileRead({:?}): bytes_per_frame is zero.",
            in_ext_audio_file
        );
        env.mem.write(io_number_frames, 0);
        return kAudioFileBadPropertySizeError;
    }
    let Some(number_of_bytes) = number_frames.checked_mul(bytes_per_frame) else {
        log!(
            "Warning: ExtAudioFileRead({:?}): {} frames * {} bytes_per_frame \
             overflows u32; clamping to 0.",
            in_ext_audio_file,
            number_frames,
            bytes_per_frame
        );
        env.mem.write(io_number_frames, 0);
        return kAudioFileBadPropertySizeError;
    };
    let number_of_bytes_ptr = env.mem.alloc_and_write(number_of_bytes);

    let res = AudioFileReadBytes(
        env,
        host_object.guest_audio_file,
        false,
        host_object.current_bytes_read,
        number_of_bytes_ptr,
        audio_buffer_list.buffers[0].data,
    );

    let number_of_bytes_read = env.mem.read(number_of_bytes_ptr);
    env.mem.free(number_of_bytes_ptr.cast());

    if res != 0 {
        if res == eofErr {
            env.mem.write(io_number_frames, 0);
            return 0;
        }
        log!(
            "Ошибка: ExtAudioFileRead({:?}, {:?}, {:?}) завершилась с кодом: {:?}",
            in_ext_audio_file,
            io_number_frames,
            io_data,
            res
        );
        return res;
    }

    let frames_read = number_of_bytes_read
        .checked_div(bytes_per_frame)
        .unwrap_or(0);
    env.mem.write(io_number_frames, frames_read);
    audio_buffer_list.buffers[0].data_byte_size = number_of_bytes_read;

    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get_mut(&in_ext_audio_file)
    else {
        // The guest disposed the file mid-read; just bail out.
        env.mem.write(io_data, audio_buffer_list);
        return 0;
    };
    host_object.current_bytes_read += number_of_bytes_read as i64;

    env.mem.write(io_data, audio_buffer_list);

    0 // успех
}

fn ExtAudioFileDispose(env: &mut Environment, in_ext_audio_file: ExtAudioFileRef) -> OSStatus {
    return_if_null!(in_ext_audio_file);

    let Some(host_object) = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .get(&in_ext_audio_file)
    else {
        log!(
            "Warning: ExtAudioFileDispose({:?}): already disposed.",
            in_ext_audio_file
        );
        return 0;
    };

    let res = AudioFileClose(env, host_object.guest_audio_file);
    if res != 0 {
        log!(
            "Warning: ExtAudioFileDispose: AudioFileClose returned {}; \
             continuing with dispose.",
            res
        );
    }

    let _host_object = env
        .framework_state
        .audio_toolbox
        .ext_audio_file
        .extended_audio_files
        .remove(&in_ext_audio_file);
    env.mem.free(in_ext_audio_file.cast());

    0 // успех
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(ExtAudioFileOpenURL(_, _)),
    export_c_func!(ExtAudioFileGetProperty(_, _, _, _)),
    export_c_func!(ExtAudioFileSetProperty(_, _, _, _)),
    export_c_func!(ExtAudioFileRead(_, _, _)),
    export_c_func!(ExtAudioFileDispose(_)),
];
