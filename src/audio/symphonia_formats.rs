/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Quick-and-dirty decoding of miscellaneous formats (MP3, AAC, CAF) to linear PCM.
//!
//! This should be the only module in HyperHLE that makes use of [symphonia].

use std::io::Cursor;
use symphonia::core::audio::AudioSpec;
use symphonia::core::io::MediaSourceStream;

/// PCM data decoded from an miscellaneous format file.
pub struct SymphoniaDecodedToPcm {
    /// 16-bit little-endian PCM samples, grouped in frames (one sample per
    /// channel in each frame).
    pub bytes: Vec<u8>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u32,
}

pub fn decode_symphonia_to_pcm(file: Cursor<Vec<u8>>) -> Result<SymphoniaDecodedToPcm, ()> {
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Пробуем определить формат. Если не вышло - логируем реальную причину.
    let mut probed = match symphonia::default::get_probe()
        .probe(&Default::default(), mss, Default::default(), Default::default()) {
        Ok(p) => p,
        Err(e) => {
            log!("Symphonia probe failed: {:?}", e);
            return Err(());
        }
    };

    // Находим любую аудиодорожку, проверяя наличие параметров аудио.
    let track = probed
        .tracks()
        .iter()
        .find(|t| {
            if let Some(codec_params) = &t.codec_params {
                codec_params.audio().is_some()
            } else {
                false
            }
        })
        .ok_or_else(|| {
            log!("Symphonia: No supported audio tracks found in file");
        })?;

    let track_id = track.id;
    
    // Получаем AudioCodecParameters, так как мы уже убедились, что они есть.
    let audio_codec_params = track.codec_params.as_ref().unwrap().audio().unwrap();

    // Создаем декодер
    let mut decoder = match symphonia::default::get_codecs()
        .make_audio_decoder(audio_codec_params, &Default::default()) {
        Ok(d) => d,
        Err(e) => {
            log!("Symphonia failed to create audio decoder: {:?}", e);
            return Err(());
        }
    };

    let mut out_pcm = Vec::<u8>::new();
    let mut audio_spec: Option<AudioSpec> = None;

    {
        let mut tmp_raw_s16_buf: Option<Vec<u8>> = None;

        loop {
            let packet = match probed.next_packet() {
                Ok(Some(packet)) => packet, // Успешно получили пакет
                Ok(None) => break,          // Конец файла (медиа завершилось)
                Err(symphonia::core::errors::Error::IoError(_)) => break,
                Err(e) => {
                    log!("Symphonia packet read error: {:?} (Stopping read but keeping decoded audio)", e);
                    break;
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            let decoded_packet = match decoder.decode(&packet) {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    // Ошибки декодирования (битый фрейм MP3/AAC) можно игнорировать и идти дальше
                    log!("Symphonia decode error (recoverable): {:?}", e);
                    continue;
                }
                Err(e) => {
                    log!("Symphonia fatal decode error: {:?}", e);
                    break;
                }
            };

            let audio_spec = audio_spec.get_or_insert_with(|| decoded_packet.spec().clone());

            let tmp_raw_s16_buf = tmp_raw_s16_buf
                .get_or_insert_with(|| Vec::with_capacity(decoded_packet.capacity()));

            tmp_raw_s16_buf.clear();
            decoded_packet.copy_bytes_to_vec_interleaved_as::<i16>(tmp_raw_s16_buf);

            out_pcm.extend_from_slice(tmp_raw_s16_buf);
        }
    }

    let audio_spec = audio_spec.ok_or_else(|| {
        log!("Symphonia: File yielded no valid audio data");
    })?;

    Ok(SymphoniaDecodedToPcm {
        bytes: out_pcm,
        sample_rate: audio_spec.rate(),
        channels: audio_spec.channels().count().try_into().unwrap(),
    })
}
