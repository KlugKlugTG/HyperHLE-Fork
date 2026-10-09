/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! AVFoundation video playback classes: `AVAsset` / `AVURLAsset`,
//! `AVPlayerItem`, `AVPlayer` and `AVPlayerLayer`.
//!
//! AVPlayer assets are demuxed with Symphonia and H.264 frames are decoded
//! with the same OpenH264 pipeline used by MPMoviePlayerController. Frames
//! are presented through AVPlayerLayer's Core Animation backing store.

use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::{ns_string, ns_url, NSUInteger};
use crate::frameworks::media_player::movie_video::{
    media_tracks, MovieMediaTrackInfo, MovieMediaType, MovieVideo, MovieVideoInfo,
};
use crate::frameworks::media_toolbox::CMTime;
use crate::mem::MutPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports,
    HostObject, NSZonePtr,
};
use crate::Environment;
use std::time::Instant;

#[derive(Default)]
struct AVAssetHostObject {
    url: id,
    duration: CMTime,
    video_info: Option<MovieVideoInfo>,
    tracks: Vec<MovieMediaTrackInfo>,
}
impl HostObject for AVAssetHostObject {}

struct AVAssetTrackHostObject {
    asset: id,
    track_id: u32,
    media_type: MovieMediaType,
}

impl Default for AVAssetTrackHostObject {
    fn default() -> Self {
        Self {
            asset: nil,
            track_id: 0,
            media_type: MovieMediaType::Audio,
        }
    }
}
impl HostObject for AVAssetTrackHostObject {}

#[derive(Default)]
struct AVAudioMixHostObject {
    input_parameters: id,
}
impl HostObject for AVAudioMixHostObject {}

struct AVAudioMixInputParametersHostObject {
    track: id,
    volume: f32,
}

impl Default for AVAudioMixInputParametersHostObject {
    fn default() -> Self {
        Self {
            track: nil,
            volume: 1.0,
        }
    }
}
impl HostObject for AVAudioMixInputParametersHostObject {}

#[derive(Default)]
struct AVPlayerItemHostObject {
    asset: id,
    audio_mix: id,
    current_time: CMTime,
    duration: CMTime,
    status: i32,
}
impl HostObject for AVPlayerItemHostObject {}

struct AVPlayerHostObject {
    current_item: id,
    rate: f32,
    muted: bool,
    volume: f32,
}

impl Default for AVPlayerHostObject {
    fn default() -> Self {
        Self {
            current_item: nil,
            rate: 0.0,
            muted: false,
            volume: 1.0,
        }
    }
}

impl HostObject for AVPlayerHostObject {}

pub(crate) struct Playback {
    item: id,
    video: Option<MovieVideo>,
    audio_player: id,
    offset: f64,
    started_at: Option<Instant>,
    duration: f64,
}

impl Playback {
    fn current_time(&self, rate: f32) -> f64 {
        let elapsed = self
            .started_at
            .map_or(0.0, |started| started.elapsed().as_secs_f64());
        (self.offset + elapsed * f64::from(rate)).clamp(0.0, self.duration)
    }
}

fn finish_notification(env: &mut Environment, item: id) {
    let name = ns_string::get_static_str(env, "AVPlayerItemDidPlayToEndTimeNotification");
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    () = msg![env; center postNotificationName:name object:item];
}

fn stop_playback(env: &mut Environment, player: id) {
    let playback = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .remove(&player);
    if let Some(playback) = playback {
        if playback.audio_player != nil {
            let audio_player = playback.audio_player;
            () = msg![env; audio_player stop];
            release(env, audio_player);
        }
    }
}

fn player_item_url(env: &Environment, item: id) -> id {
    if item == nil {
        return nil;
    }
    let asset = env.objc.borrow::<AVPlayerItemHostObject>(item).asset;
    if asset == nil {
        return nil;
    }
    env.objc.borrow::<AVAssetHostObject>(asset).url
}

fn asset_tracks(
    env: &mut Environment,
    asset: id,
    media_type_filter: Option<MovieMediaType>,
) -> id {
    let tracks = env.objc.borrow::<AVAssetHostObject>(asset).tracks.clone();
    let result: id = msg_class![env; NSMutableArray array];
    for track_info in tracks {
        if media_type_filter.is_some_and(|media_type| media_type != track_info.media_type) {
            continue;
        }
        let media_type = match track_info.media_type {
            MovieMediaType::Audio => ns_string::get_static_str(env, "soun"),
            MovieMediaType::Video => ns_string::get_static_str(env, "vide"),
        };
        let track_id = track_info.track_id;
        let track: id = msg_class![env; AVAssetTrack alloc];
        let track: id = msg![env; track initWithAsset:asset trackID:track_id mediaType:media_type];
        if track != nil {
            () = msg![env; result addObject:track];
            release(env, track);
        }
    }
    result
}

fn make_audio_player(env: &mut Environment, bytes: &[u8]) -> id {
    let Ok(length) = NSUInteger::try_from(bytes.len()) else {
        return nil;
    };
    let buffer = env.mem.alloc(length);
    env.mem
        .bytes_at_mut(buffer.cast(), length as crate::mem::GuestUSize)
        .copy_from_slice(bytes);
    let buffer_ptr = buffer.cast_const().cast_void();
    let data: id = msg_class![env; NSData dataWithBytes:buffer_ptr length:length];
    env.mem.free(buffer.cast());
    if data == nil {
        return nil;
    }
    let audio_player: id = msg_class![env; AVAudioPlayer alloc];
    msg![env; audio_player initWithData:data error:(MutPtr::<id>::null())]
}

fn effective_audio_volume(env: &mut Environment, item: id, volume: f32, muted: bool) -> f32 {
    if muted {
        return 0.0;
    }
    if item == nil {
        return volume;
    }
    let audio_mix = env.objc.borrow::<AVPlayerItemHostObject>(item).audio_mix;
    if audio_mix == nil {
        return volume;
    }
    let parameters = env
        .objc
        .borrow::<AVAudioMixHostObject>(audio_mix)
        .input_parameters;
    if parameters == nil {
        return volume;
    }
    let count: NSUInteger = msg![env; parameters count];
    if count == 0 {
        return volume;
    }
    let input_parameters: id = msg![env; parameters objectAtIndex:0u32];
    if input_parameters == nil {
        return volume;
    }
    let mix_volume = env
        .objc
        .borrow::<AVAudioMixInputParametersHostObject>(input_parameters)
        .volume;
    if mix_volume.is_finite() {
        volume * mix_volume.clamp(0.0, 1.0)
    } else {
        volume
    }
}

fn start_audio(
    env: &mut Environment,
    audio_player: id,
    item: id,
    position: f64,
    volume: f32,
    muted: bool,
) {
    if audio_player == nil {
        return;
    }
    let volume = effective_audio_volume(env, item, volume, muted);
    () = msg![env; audio_player setVolume:volume];
    () = msg![env; audio_player prepareToPlay];
    () = msg![env; audio_player setCurrentTime:position];
    let _started: bool = msg![env; audio_player play];
}

fn start_playback(env: &mut Environment, player: id) -> bool {
    let item = env.objc.borrow::<AVPlayerHostObject>(player).current_item;
    if item == nil {
        return false;
    }
    let existing = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .get_mut(&player)
        .and_then(|playback| {
            if playback.item == item {
                playback.started_at = Some(Instant::now());
                Some((playback.audio_player, playback.offset))
            } else {
                None
            }
        });
    if let Some((audio_player, position)) = existing {
        let (volume, muted) = {
            let host = env.objc.borrow::<AVPlayerHostObject>(player);
            (host.volume, host.muted)
        };
        start_audio(env, audio_player, item, position, volume, muted);
        return true;
    }
    stop_playback(env, player);

    let url = player_item_url(env, item);
    if url == nil {
        return false;
    }
    let path = ns_url::to_rust_path(env, url);
    let Ok(bytes) = env.fs.read(&path) else {
        log!("AVPlayer: couldn't read media at {:?}", path.as_str());
        return false;
    };
    let Some(video) = MovieVideo::start(&bytes, false) else {
        log!("AVPlayer: media has no supported H.264 video track");
        return false;
    };
    let duration = crate::frameworks::media_player::mp4_duration(&bytes)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .unwrap_or_else(|| {
            env.objc
                .borrow::<AVPlayerItemHostObject>(item)
                .duration
                .as_seconds()
        });
    if !duration.is_finite() || duration <= 0.0 {
        return false;
    }
    let (volume, muted) = {
        let host = env.objc.borrow::<AVPlayerHostObject>(player);
        (host.volume, host.muted)
    };
    let audio_player = make_audio_player(env, &bytes);
    let mut offset = env
        .objc
        .borrow::<AVPlayerItemHostObject>(item)
        .current_time
        .as_seconds();
    if !offset.is_finite() || offset < 0.0 || offset >= duration {
        offset = 0.0;
    }
    env.objc.borrow_mut::<AVPlayerItemHostObject>(item).duration =
        CMTime::from_seconds_f64(duration);
    env.objc
        .borrow_mut::<AVPlayerItemHostObject>(item)
        .current_time = CMTime::from_seconds_f64(offset);
    env.framework_state.avfoundation.av_player_playbacks.insert(
        player,
        Playback {
            item,
            video: Some(video),
            audio_player,
            offset,
            started_at: Some(Instant::now()),
            duration,
        },
    );
    start_audio(env, audio_player, item, offset, volume, muted);
    true
}

fn pause_playback(env: &mut Environment, player: id) {
    let rate = env.objc.borrow::<AVPlayerHostObject>(player).rate;
    let (item, audio_player, position) = {
        let Some(playback) = env
            .framework_state
            .avfoundation
            .av_player_playbacks
            .get_mut(&player)
        else {
            return;
        };
        let position = playback.current_time(rate);
        playback.offset = position;
        playback.started_at = None;
        (playback.item, playback.audio_player, position)
    };
    env.objc
        .borrow_mut::<AVPlayerItemHostObject>(item)
        .current_time = CMTime::from_seconds_f64(position);
    if audio_player != nil {
        () = msg![env; audio_player pause];
    }
}

pub(crate) fn has_active_video(env: &mut Environment) -> bool {
    env.framework_state
        .avfoundation
        .av_player_playbacks
        .values()
        .any(|playback| playback.video.is_some() && playback.started_at.is_some())
}

pub(crate) fn handle_players(env: &mut Environment) {
    let players: Vec<id> = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .keys()
        .copied()
        .collect();
    for player in players {
        let rate = env.objc.borrow::<AVPlayerHostObject>(player).rate;
        let (item, position, duration, frame) = {
            let Some(playback) = env
                .framework_state
                .avfoundation
                .av_player_playbacks
                .get_mut(&player)
            else {
                continue;
            };
            let position = playback.current_time(rate);
            let frame = playback
                .video
                .as_mut()
                .and_then(|video| video.take_frame(position));
            (playback.item, position, playback.duration, frame)
        };
        env.objc
            .borrow_mut::<AVPlayerItemHostObject>(item)
            .current_time = CMTime::from_seconds_f64(position);
        if let Some(frame) = frame {
            let layers: Vec<id> = env
                .framework_state
                .avfoundation
                .av_player_layer_players
                .iter()
                .filter_map(|(layer, candidate)| (*candidate == player).then_some(*layer))
                .collect();
            for layer in layers {
                let newly_ready = env
                    .framework_state
                    .avfoundation
                    .av_player_layers_ready
                    .insert(layer);
                if newly_ready {
                    let key = ns_string::get_static_str(env, "readyForDisplay");
                    () = msg![env; layer willChangeValueForKey:key];
                }
                crate::frameworks::core_animation::ca_eagl_layer::present_pixels(
                    env,
                    layer,
                    frame.rgba.clone(),
                    frame.width,
                    frame.height,
                );
                if newly_ready {
                    let key = ns_string::get_static_str(env, "readyForDisplay");
                    () = msg![env; layer didChangeValueForKey:key];
                }
            }
        }
        if rate > 0.0 && position >= duration {
            stop_playback(env, player);
            env.objc.borrow_mut::<AVPlayerHostObject>(player).rate = 0.0;
            env.objc
                .borrow_mut::<AVPlayerItemHostObject>(item)
                .current_time = CMTime::from_seconds_f64(duration);
            finish_notification(env, item);
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// ===========================================================================
// AVAsset / AVURLAsset
// ===========================================================================

@implementation AVAsset: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVAssetHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (())dealloc {
    let url = env.objc.borrow::<AVAssetHostObject>(this).url;
    release(env, url);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)tracks {
    asset_tracks(env, this, None)
}
- (id)commonMetadata {
    msg_class![env; NSArray array]
}
- (CMTime)duration {
    env.objc.borrow::<AVAssetHostObject>(this).duration
}
- (bool)isPlayable { true }
- (bool)isReadable { true }
- (bool)isExportable { false }
- (bool)hasProtectedContent { false }

- (id)tracksWithMediaType:(id)media_type {
    match ns_string::to_rust_string(env, media_type).as_ref() {
        "vide" => asset_tracks(env, this, Some(MovieMediaType::Video)),
        "soun" => asset_tracks(env, this, Some(MovieMediaType::Audio)),
        _ => msg_class![env; NSArray array],
    }
}

- (id)tracksWithMediaCharacteristic:(id)characteristic {
    match ns_string::to_rust_string(env, characteristic).as_ref() {
        "public.visual" => asset_tracks(env, this, Some(MovieMediaType::Video)),
        "public.audible" => asset_tracks(env, this, Some(MovieMediaType::Audio)),
        _ => msg_class![env; NSArray array],
    }
}

- (())loadValuesAsynchronouslyForKeys:(id)_keys
                    completionHandler:(id)handler {
    // Report completion immediately so apps that gate startup on the
    // callback don't wait forever.
    if handler != nil {
        () = msg![env; handler invoke];
    }
}

- (i32)statusOfValueForKey:(id)_key error:(crate::mem::MutPtr<id>)error {
    if !error.is_null() {
        env.mem.write(error, nil);
    }
    // AVKeyValueStatusLoaded = 2
    2
}

@end

@implementation AVURLAsset: AVAsset

+ (id)URLAssetWithURL:(id)url options:(id)_options { // NSURL*, NSDictionary*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithURL:url options:_options];
    autorelease(env, new)
}

- (id)initWithURL:(id)url options:(id)_options {
    let this: id = msg_super![env; this init];
    if url != nil {
        retain(env, url);
    }
    let (duration, video_info, tracks) = if url != nil {
        let path = ns_url::to_rust_path(env, url);
        match env.fs.read(&path) {
            Ok(bytes) => {
                let duration = crate::frameworks::media_player::mp4_duration(&bytes)
                    .filter(|duration| duration.is_finite() && *duration > 0.0)
                    .map(CMTime::from_seconds_f64)
                    .unwrap_or_default();
                (
                    duration,
                    MovieVideo::probe(&bytes),
                    media_tracks(&bytes),
                )
            }
            Err(_) => (CMTime::default(), None, Vec::new()),
        }
    } else {
        (CMTime::default(), None, Vec::new())
    };
    let host = env.objc.borrow_mut::<AVAssetHostObject>(this);
    host.url = url;
    host.duration = duration;
    host.video_info = video_info;
    host.tracks = tracks;
    this
}

- (id)URL {
    env.objc.borrow::<AVAssetHostObject>(this).url
}

@end

@implementation AVAssetTrack: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVAssetTrackHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (id)initWithAsset:(id)asset trackID:(u32)track_id mediaType:(id)media_type {
    let this: id = msg_super![env; this init];
    if asset != nil {
        retain(env, asset);
    }
    let media_type = match ns_string::to_rust_string(env, media_type).as_ref() {
        "vide" => MovieMediaType::Video,
        _ => MovieMediaType::Audio,
    };
    let host = env.objc.borrow_mut::<AVAssetTrackHostObject>(this);
    host.asset = asset;
    host.track_id = track_id;
    host.media_type = media_type;
    this
}

- (())dealloc {
    let asset = env.objc.borrow::<AVAssetTrackHostObject>(this).asset;
    release(env, asset);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)asset {
    env.objc.borrow::<AVAssetTrackHostObject>(this).asset
}
- (u32)trackID {
    env.objc.borrow::<AVAssetTrackHostObject>(this).track_id
}
- (id)mediaType {
    let media_type = env.objc.borrow::<AVAssetTrackHostObject>(this).media_type;
    let media_type = match media_type {
        MovieMediaType::Audio => "soun",
        MovieMediaType::Video => "vide",
    };
    ns_string::get_static_str(env, media_type)
}
- (CGSize)naturalSize {
    let host = env.objc.borrow::<AVAssetTrackHostObject>(this);
    if host.media_type == MovieMediaType::Video && host.asset != nil {
        if let Some(video) = env.objc.borrow::<AVAssetHostObject>(host.asset).video_info {
            return CGSize {
                width: video.width as f32,
                height: video.height as f32,
            };
        }
    }
    CGSize::default()
}
- (CMTime)duration {
    let asset = env.objc.borrow::<AVAssetTrackHostObject>(this).asset;
    if asset == nil {
        CMTime::default()
    } else {
        env.objc.borrow::<AVAssetHostObject>(asset).duration
    }
}
- (bool)isPlayable { true }
- (bool)isEnabled { true }

@end

@implementation AVMutableAudioMixInputParameters: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVAudioMixInputParametersHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)audioMixInputParametersWithTrack:(id)track {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithTrack:track];
    autorelease(env, new)
}

- (id)initWithTrack:(id)track {
    let this: id = msg_super![env; this init];
    if track != nil {
        retain(env, track);
    }
    env.objc.borrow_mut::<AVAudioMixInputParametersHostObject>(this).track = track;
    this
}

- (())dealloc {
    let track = env
        .objc
        .borrow::<AVAudioMixInputParametersHostObject>(this)
        .track;
    release(env, track);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)track {
    env.objc.borrow::<AVAudioMixInputParametersHostObject>(this).track
}
- (i32)trackID {
    let track = env.objc.borrow::<AVAudioMixInputParametersHostObject>(this).track;
    if track == nil {
        0
    } else {
        let track_id: u32 = msg![env; track trackID];
        track_id as i32
    }
}
- (())setVolume:(f32)volume atTime:(CMTime)_time {
    env.objc
        .borrow_mut::<AVAudioMixInputParametersHostObject>(this)
        .volume = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    };
}

@end

@implementation AVMutableAudioMix: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVAudioMixHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)audioMix {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    autorelease(env, new)
}

- (())dealloc {
    let input_parameters = env.objc.borrow::<AVAudioMixHostObject>(this).input_parameters;
    release(env, input_parameters);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)inputParameters {
    env.objc
        .borrow::<AVAudioMixHostObject>(this)
        .input_parameters
}
- (())setInputParameters:(id)input_parameters {
    let old = env.objc.borrow::<AVAudioMixHostObject>(this).input_parameters;
    if old == input_parameters {
        return;
    }
    if input_parameters != nil {
        retain(env, input_parameters);
    }
    release(env, old);
    env.objc
        .borrow_mut::<AVAudioMixHostObject>(this)
        .input_parameters = input_parameters;
}

@end


// ===========================================================================
// AVPlayerItem
// ===========================================================================

@implementation AVPlayerItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVPlayerItemHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)playerItemWithAsset:(id)asset {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithAsset:asset];
    autorelease(env, new)
}

+ (id)playerItemWithURL:(id)url {
    let asset: id = msg_class![env; AVURLAsset URLAssetWithURL:url options:nil];
    msg![env; this playerItemWithAsset:asset]
}

- (id)initWithAsset:(id)asset {
    if asset != nil {
        retain(env, asset);
    }
    let duration = if asset == nil {
        CMTime::default()
    } else {
        env.objc.borrow::<AVAssetHostObject>(asset).duration
    };
    let host = env.objc.borrow_mut::<AVPlayerItemHostObject>(this);
    host.asset = asset;
    host.duration = duration;
    host.current_time = CMTime::from_seconds_f64(0.0);
    this
}

- (id)initWithURL:(id)url {
    let asset: id = msg_class![env; AVURLAsset URLAssetWithURL:url options:nil];
    msg![env; this initWithAsset:asset]
}

- (())dealloc {
    let (asset, audio_mix) = {
        let host = env.objc.borrow::<AVPlayerItemHostObject>(this);
        (host.asset, host.audio_mix)
    };
    release(env, asset);
    release(env, audio_mix);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)asset {
    env.objc.borrow::<AVPlayerItemHostObject>(this).asset
}

- (CMTime)currentTime {
    env.objc.borrow::<AVPlayerItemHostObject>(this).current_time
}

- (CMTime)duration {
    env.objc.borrow::<AVPlayerItemHostObject>(this).duration
}

- (())seekToTime:(CMTime)time {
    env.objc.borrow_mut::<AVPlayerItemHostObject>(this).current_time = time;
}

- (())addObserver:(id)observer forKeyPath:(id)key_path options:(u32)options context:(id)context {
    () = msg_super![env; this addObserver:observer forKeyPath:key_path options:options context:context];
}

- (i32)status {
    env.objc.borrow::<AVPlayerItemHostObject>(this).status
}

- (id)valueForKey:(id)key {
    if ns_string::to_rust_string(env, key) == "status" {
        let status = env.objc.borrow::<AVPlayerItemHostObject>(this).status;
        return msg_class![env; NSNumber numberWithInt:status];
    }
    msg_super![env; this valueForKey:key]
}

- (id)tracks {
    msg_class![env; NSArray array]
}

- (id)audioMix {
    env.objc.borrow::<AVPlayerItemHostObject>(this).audio_mix
}

- (())setAudioMix:(id)audio_mix {
    let old = env.objc.borrow::<AVPlayerItemHostObject>(this).audio_mix;
    if old == audio_mix {
        return;
    }
    if audio_mix != nil {
        retain(env, audio_mix);
    }
    release(env, old);
    env.objc.borrow_mut::<AVPlayerItemHostObject>(this).audio_mix = audio_mix;
}

- (())addOutput:(id)_output {}
- (())removeOutput:(id)_output {}

@end

// ===========================================================================
// AVPlayer
// ===========================================================================

@implementation AVPlayer: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVPlayerHostObject::default());
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)playerWithPlayerItem:(id)item {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithPlayerItem:item];
    autorelease(env, new)
}

+ (id)playerWithURL:(id)url {
    let item: id = msg_class![env; AVPlayerItem playerItemWithURL:url];
    msg![env; this playerWithPlayerItem:item]
}

- (id)initWithPlayerItem:(id)item {
    if item != nil {
        retain(env, item);
    }
    env.objc.borrow_mut::<AVPlayerHostObject>(this).current_item = item;
    this
}

- (id)initWithURL:(id)url {
    let item: id = msg_class![env; AVPlayerItem playerItemWithURL:url];
    msg![env; this initWithPlayerItem:item]
}

- (())dealloc {
    stop_playback(env, this);
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    release(env, item);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)currentItem {
    env.objc.borrow::<AVPlayerHostObject>(this).current_item
}

- (CMTime)currentTime {
    if let Some(playback) = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .get(&this)
    {
        let rate = env.objc.borrow::<AVPlayerHostObject>(this).rate;
        return CMTime::from_seconds_f64(playback.current_time(rate));
    }
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item == nil {
        CMTime::default()
    } else {
        env.objc.borrow::<AVPlayerItemHostObject>(item).current_time
    }
}

- (())seekToTime:(CMTime)time {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item == nil {
        return;
    }
    let duration = env
        .objc
        .borrow::<AVPlayerItemHostObject>(item)
        .duration
        .as_seconds();
    let requested = time.as_seconds();
    let position = if requested.is_finite() && requested >= 0.0 {
        if duration.is_finite() && duration > 0.0 {
            requested.min(duration)
        } else {
            requested
        }
    } else {
        0.0
    };
    let audio_player = if let Some(playback) = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .get_mut(&this)
    {
        playback.offset = position;
        playback.started_at = (env.objc.borrow::<AVPlayerHostObject>(this).rate > 0.0)
            .then(Instant::now);
        playback.audio_player
    } else {
        nil
    };
    if audio_player != nil {
        () = msg![env; audio_player setCurrentTime:position];
    }
    env.objc
        .borrow_mut::<AVPlayerItemHostObject>(item)
        .current_time = CMTime::from_seconds_f64(position);
}

- (())replaceCurrentItemWithPlayerItem:(id)item {
    stop_playback(env, this);
    let old = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item != nil {
        retain(env, item);
    }
    release(env, old);
    env.objc.borrow_mut::<AVPlayerHostObject>(this).current_item = item;
}

// AVPlayerStatusReadyToPlay = 1
- (i32)status { 1 }

- (f32)rate {
    env.objc.borrow::<AVPlayerHostObject>(this).rate
}
- (())setRate:(f32)rate {
    let rate = if rate.is_finite() { rate.max(0.0) } else { 0.0 };
    if rate == 0.0 {
        pause_playback(env, this);
        env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 0.0;
    } else {
        env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = rate;
        if !start_playback(env, this) {
            env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 0.0;
        }
    }
}

- (())setActionAtItemEnd:(i64)_action {}
- (())setMuted:(bool)muted {
    let (volume, item) = {
        let host = env.objc.borrow_mut::<AVPlayerHostObject>(this);
        host.muted = muted;
        (host.volume, host.current_item)
    };
    let audio_player = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .get(&this)
        .map_or(nil, |playback| playback.audio_player);
    if audio_player != nil {
        let volume = effective_audio_volume(env, item, volume, muted);
        () = msg![env; audio_player setVolume:volume];
    }
}
- (bool)isMuted { env.objc.borrow::<AVPlayerHostObject>(this).muted }
- (())setVolume:(f32)volume {
    let (volume, muted, item) = {
        let host = env.objc.borrow_mut::<AVPlayerHostObject>(this);
        host.volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        (host.volume, host.muted, host.current_item)
    };
    let audio_player = env
        .framework_state
        .avfoundation
        .av_player_playbacks
        .get(&this)
        .map_or(nil, |playback| playback.audio_player);
    if audio_player != nil {
        let volume = effective_audio_volume(env, item, volume, muted);
        () = msg![env; audio_player setVolume:volume];
    }
}
- (f32)volume { env.objc.borrow::<AVPlayerHostObject>(this).volume }

- (())play {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item == nil {
        return;
    }
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 1.0;
    if !start_playback(env, this) {
        let selector = env.objc.register_host_selector(
            "_touchHLE_finishPlaybackForItem:".to_string(),
            &mut env.mem,
        );
        () = msg![env; this performSelector:selector withObject:item afterDelay:0.0_f64];
    }
}

- (())_touchHLE_finishPlaybackForItem:(id)item {
    if item == nil {
        return;
    }
    let duration = env.objc.borrow::<AVPlayerItemHostObject>(item).duration;
    env.objc
        .borrow_mut::<AVPlayerItemHostObject>(item)
        .current_time = duration;
    let name = ns_string::get_static_str(env, "AVPlayerItemDidPlayToEndTimeNotification");
    let nc: id = msg_class![env; NSNotificationCenter defaultCenter];
    () = msg![env; nc postNotificationName:name object:item];
}

- (())pause {
    pause_playback(env, this);
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 0.0;
}

@end

// ===========================================================================
// AVPlayerLayer (CALayer subclass)
// ===========================================================================

@implementation AVPlayerLayer: CALayer

+ (id)playerLayerWithPlayer:(id)player {
    let cls: crate::objc::Class =
        env.objc.get_known_class("AVPlayerLayer", &mut env.mem);
    let layer: id = msg![env; cls alloc];
    let layer: id = msg![env; layer init];
    () = msg![env; layer setPlayer:player];
    // Keep the layer black until its first decoded frame is presented.
    let black: id = msg_class![env; UIColor blackColor];
    let cg: id = msg![env; black CGColor];
    () = msg![env; layer setBackgroundColor:cg];
    autorelease(env, layer)
}

- (id)player {
    env.framework_state
        .avfoundation
        .av_player_layer_players
        .get(&this)
        .copied()
        .unwrap_or(nil)
}
- (())setPlayer:(id)player {
    let old = env
        .framework_state
        .avfoundation
        .av_player_layer_players
        .get(&this)
        .copied()
        .unwrap_or(nil);
    if player != nil {
        retain(env, player);
    }
    if old != nil {
        release(env, old);
    }
    env.framework_state
        .avfoundation
        .av_player_layers_ready
        .remove(&this);
    if player == nil {
        env.framework_state
            .avfoundation
            .av_player_layer_players
            .remove(&this);
        return;
    }
    env.framework_state
        .avfoundation
        .av_player_layer_players
        .insert(this, player);
    if player != nil {
        let item = env.objc.borrow::<AVPlayerHostObject>(player).current_item;
        if item != nil && env.objc.borrow::<AVPlayerItemHostObject>(item).status == 0 {
            let status_key = ns_string::get_static_str(env, "status");
            () = msg![env; item willChangeValueForKey:status_key];
            env.objc.borrow_mut::<AVPlayerItemHostObject>(item).status = 1;
            () = msg![env; item didChangeValueForKey:status_key];
        }
    }

}

- (())dealloc {
    let player = env
        .framework_state
        .avfoundation
        .av_player_layer_players
        .remove(&this)
        .unwrap_or(nil);
    env.framework_state
        .avfoundation
        .av_player_layers_ready
        .remove(&this);
    if player != nil {
        release(env, player);
    }
    msg_super![env; this dealloc]
}

- (id)videoGravity {
    ns_string::get_static_str(env, "AVLayerVideoGravityResizeAspect")
}
- (())setVideoGravity:(id)_gravity {}

- (bool)isReadyForDisplay {
    env.framework_state
        .avfoundation
        .av_player_layers_ready
        .contains(&this)
}

- (CGRect)videoRect {
    let bounds: CGRect = msg![env; this bounds];
    let player = env
        .framework_state
        .avfoundation
        .av_player_layer_players
        .get(&this)
        .copied()
        .unwrap_or(nil);
    if player == nil {
        return bounds;
    }
    let item = env.objc.borrow::<AVPlayerHostObject>(player).current_item;
    if item == nil {
        return bounds;
    }
    let asset = env.objc.borrow::<AVPlayerItemHostObject>(item).asset;
    if asset == nil {
        return bounds;
    }
    let Some(video) = env.objc.borrow::<AVAssetHostObject>(asset).video_info else {
        return bounds;
    };
    if video.width == 0 || video.height == 0 || bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return bounds;
    }
    let video_aspect = video.width as f32 / video.height as f32;
    let bounds_aspect = bounds.size.width / bounds.size.height;
    let size = if video_aspect > bounds_aspect {
        CGSize {
            width: bounds.size.width,
            height: bounds.size.width / video_aspect,
        }
    } else {
        CGSize {
            width: bounds.size.height * video_aspect,
            height: bounds.size.height,
        }
    };
    CGRect {
        origin: CGPoint {
            x: bounds.origin.x + (bounds.size.width - size.width) * 0.5,
            y: bounds.origin.y + (bounds.size.height - size.height) * 0.5,
        },
        size,
    }
}

@end

};
