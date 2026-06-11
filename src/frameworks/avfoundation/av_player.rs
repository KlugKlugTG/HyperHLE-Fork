/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! AVPlayer / AVPlayerLayer / AVPlayerItem / AVURLAsset stubs.
//!
//! touchHLE does not ship an H.264 decoder, so we cannot render video frames.
//! However, many games (notably Infinity Blade III) create these objects to
//! play intro cutscenes. When these classes are unimplemented, all four return
//! nil (via the "faked class" path), which causes the app to dereference nil
//! pointers, cascade into NULL-page reads, corrupt internal state, and
//! eventually request a 4GB allocation that triggers the emulator's OOM guard
//! — producing a black screen after the logo.
//!
//! These stubs return valid (empty) objects so that the app's VC hierarchy
//! and rendering loop survive the cutscene setup phase. AVPlayerLayer extends
//! CALayer and acts as a transparent placeholder; the other three are
//! NSObject subclasses that hold minimal bookkeeping state.

use crate::frameworks::core_graphics::CGRect;
use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::{NSInteger, NSTimeInterval};
use crate::objc::{
    autorelease, id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports,
    HostObject, NSZonePtr,
};
use crate::Environment;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Host objects
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AVURLAssetHostObject {
    url: id,
}
impl HostObject for AVURLAssetHostObject {}

#[derive(Default)]
struct AVPlayerItemHostObject {
    asset: id,
    status: NSInteger, // AVPlayerItemStatus
}
impl HostObject for AVPlayerItemHostObject {}

// AVPlayerItemStatus values
#[allow(dead_code)]
const AVPlayerItemStatusUnknown: NSInteger = 0;
const AVPlayerItemStatusReadyToPlay: NSInteger = 1;

#[derive(Default)]
struct AVPlayerHostObject {
    current_item: id,
    rate: f32,
    muted: bool,
    volume: f32,
    action_at_item_end: NSInteger,
    /// Next scheduled action; see schedule_finish_notification.
    finish_scheduled: bool,
}
impl HostObject for AVPlayerHostObject {}

// AVPlayerActionAtItemEnd values
const AVPlayerActionAtItemEndAdvance: NSInteger = 0;
#[allow(dead_code)]
const AVPlayerActionAtItemEndPause: NSInteger = 1;
#[allow(dead_code)]
const AVPlayerActionAtItemEndNone: NSInteger = 2;

/// Side-table for AVPlayerLayer instances. See comments on
/// `av_capture::AVCapturePreviewLayerExtra` for the CALayer-subclass pattern.
#[derive(Default)]
pub struct AVPlayerLayerExtra {
    player: id,
    video_gravity: id,
    ready_for_display: bool,
}

#[derive(Default)]
pub struct State {
    /// AVPlayerLayer side-table keyed by layer `id`.
    pub player_layer_extras: HashMap<id, AVPlayerLayerExtra>,
    /// Players that have posted or are about to post finish notifications.
    pub live_players: HashMap<id, Instant>,
}

// ---------------------------------------------------------------------------
// Scheduling helpers
// ---------------------------------------------------------------------------

/// Schedule an async `AVPlayerItemDidPlayToEndTimeNotification` and
/// `AVPlayerItemFailedToPlayToEndTimeNotification` cycle. The pattern
/// mirrors what `media_player::movie_player` already does for
/// `MPMoviePlayerController`. Since we have no decoder, we post the
/// finish notification quickly so the app can move on to its main UI.
fn schedule_finish_cycle(env: &mut Environment, player: id) {
    let already = {
        let host = env.objc.borrow::<AVPlayerHostObject>(player);
        host.finish_scheduled
    };
    if already {
        return;
    }
    env.objc
        .borrow_mut::<AVPlayerHostObject>(player)
        .finish_scheduled = true;

    let state = &mut env.framework_state.avfoundation.av_player;
    state
        .live_players
        .insert(player, Instant::now() + Duration::from_millis(80));
}

/// Called every run-loop tick to dispatch pending finish notifications.
pub fn handle_players(env: &mut Environment) {
    let now = Instant::now();
    let ready: Vec<id> = {
        let state = &mut env.framework_state.avfoundation.av_player;
        let ready: Vec<id> = state
            .live_players
            .iter()
            .filter(|(_, when)| **when <= now)
            .map(|(p, _)| *p)
            .collect();
        for player in &ready {
            state.live_players.remove(player);
        }
        ready
    };

    for player in ready {
        // The item that was playing: post its end notification.
        let item = env
            .objc
            .borrow::<AVPlayerHostObject>(player)
            .current_item;
        if item != nil {
            let nc: id = msg_class![env; NSNotificationCenter defaultCenter];
            let name = ns_string::get_static_str(
                env,
                "AVPlayerItemDidPlayToEndTimeNotification",
            );
            () = msg![env; nc postNotificationName:name object:item];
        }

        // Notify AVPlayerLayer instances to flip readyForDisplay.
        mark_player_layers_ready(env, player);
    }
}

fn mark_player_layers_ready(env: &mut Environment, player: id) {
    let extras = &mut env.framework_state.avfoundation.av_player.player_layer_extras;
    for extra in extras.values_mut() {
        if extra.player == player {
            extra.ready_for_display = true;
        }
    }
}

// ---------------------------------------------------------------------------
// AVPlayerLayer side-table helpers
// ---------------------------------------------------------------------------

fn player_layer_extras(
    env: &mut Environment,
) -> &mut HashMap<id, AVPlayerLayerExtra> {
    &mut env.framework_state.avfoundation.av_player.player_layer_extras
}

// ---------------------------------------------------------------------------
// Class implementations
// ---------------------------------------------------------------------------

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// ---------------------------------------------------------------------------
// AVURLAsset
// ---------------------------------------------------------------------------
@implementation AVURLAsset: NSObject

+ (id)URLAssetWithURL:(id)url options:(id)_options {
    let cls = env.objc.get_known_class("AVURLAsset", &mut env.mem);
    let alloc: id = msg![env; cls alloc];
    let init: id = msg![env; alloc initWithURL:url options:_options];
    autorelease(env, init)
}

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::<AVURLAssetHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (id)initWithURL:(id)url options:(id)_options {
    let _: id = msg_super![env; this init];
    let url_str = if url != nil {
        ns_string::to_rust_string(env, url)
    } else {
        "(nil)".to_string().into()
    };
    log_dbg!("[(AVURLAsset*){:?} initWithURL:{} options:{:?}] (stub)", this, url_str, _options);
    if url != nil {
        retain(env, url);
    }
    env.objc.borrow_mut::<AVURLAssetHostObject>(this).url = url;
    this
}

- (())dealloc {
    let url = env.objc.borrow::<AVURLAssetHostObject>(this).url;
    if url != nil {
        release(env, url);
    }
    env.objc.dealloc_object(this, &mut env.mem);
}

- (id)URL {
    env.objc.borrow::<AVURLAssetHostObject>(this).url
}

- (NSTimeInterval)duration {
    5.0 // A plausible duration for a short intro video.
}

- (id)tracks {
    msg_class![env; NSArray array] // No real tracks.
}

@end

// ---------------------------------------------------------------------------
// AVPlayerItem
// ---------------------------------------------------------------------------
@implementation AVPlayerItem: NSObject

+ (id)playerItemWithAsset:(id)asset {
    let cls = env.objc.get_known_class("AVPlayerItem", &mut env.mem);
    let alloc: id = msg![env; cls alloc];
    let init: id = msg![env; alloc initWithAsset:asset];
    autorelease(env, init)
}

+ (id)playerItemWithURL:(id)url {
    let asset: id = msg_class![env; AVURLAsset URLAssetWithURL:url options:nil];
    msg_class![env; AVPlayerItem playerItemWithAsset:asset]
}

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::<AVPlayerItemHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (id)initWithAsset:(id)asset {
    let _: id = msg_super![env; this init];
    log_dbg!("[(AVPlayerItem*){:?} initWithAsset:{:?}] (stub)", this, asset);
    if asset != nil {
        retain(env, asset);
    }
    let host = env.objc.borrow_mut::<AVPlayerItemHostObject>(this);
    host.asset = asset;
    host.status = AVPlayerItemStatusReadyToPlay;
    this
}

- (())dealloc {
    let asset = env.objc.borrow::<AVPlayerItemHostObject>(this).asset;
    if asset != nil {
        release(env, asset);
    }
    env.objc.dealloc_object(this, &mut env.mem);
}

- (id)asset {
    env.objc.borrow::<AVPlayerItemHostObject>(this).asset
}

- (NSInteger)status {
    env.objc.borrow::<AVPlayerItemHostObject>(this).status
}

- (bool)isPlaybackBufferEmpty {
    false
}

- (bool)isPlaybackLikelyToKeepUp {
    true
}

@end

// ---------------------------------------------------------------------------
// AVPlayer
// ---------------------------------------------------------------------------
@implementation AVPlayer: NSObject

+ (id)playerWithPlayerItem:(id)item {
    let cls = env.objc.get_known_class("AVPlayer", &mut env.mem);
    let alloc: id = msg![env; cls alloc];
    let init: id = msg![env; alloc initWithPlayerItem:item];
    autorelease(env, init)
}

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(AVPlayerHostObject {
        current_item: nil,
        rate: 0.0,
        muted: false,
        volume: 1.0,
        action_at_item_end: AVPlayerActionAtItemEndAdvance,
        finish_scheduled: false,
    });
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (id)initWithPlayerItem:(id)item {
    let _: id = msg_super![env; this init];
    log_dbg!("[(AVPlayer*){:?} initWithPlayerItem:{:?}] (stub)", this, item);
    () = msg![env; this replaceCurrentItemWithPlayerItem:item];
    this
}

- (())dealloc {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item != nil {
        release(env, item);
    }
    env.framework_state.avfoundation.av_player.live_players.remove(&this);
    env.objc.dealloc_object(this, &mut env.mem);
}

- (())replaceCurrentItemWithPlayerItem:(id)item {
    let old = {
        let host = env.objc.borrow_mut::<AVPlayerHostObject>(this);
        let old = host.current_item;
        host.current_item = item;
        host.finish_scheduled = false;
        old
    };
    if item != nil {
        retain(env, item);
    }
    if old != nil {
        release(env, old);
    }
    env.framework_state.avfoundation.av_player.live_players.remove(&this);
}

- (id)currentItem {
    env.objc.borrow::<AVPlayerHostObject>(this).current_item
}

- (())play {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    if item == nil {
        log!("Warning: [(AVPlayer*){:?} play] called with no current item; ignoring.", this);
        return;
    }
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 1.0;
    // Schedule the async finish — no actual decoder, so we post it quickly.
    schedule_finish_cycle(env, this);
}

- (())pause {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 0.0;
    env.framework_state.avfoundation.av_player.live_players.remove(&this);
}

- (bool)isMuted {
    env.objc.borrow::<AVPlayerHostObject>(this).muted
}

- (())setMuted:(bool)muted {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).muted = muted;
}

- (f32)volume {
    env.objc.borrow::<AVPlayerHostObject>(this).volume
}

- (())setVolume:(f32)volume {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).volume = volume;
}

- (f32)rate {
    env.objc.borrow::<AVPlayerHostObject>(this).rate
}

- (())setRate:(f32)rate {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = rate;
    if rate > 0.0 {
        schedule_finish_cycle(env, this);
    } else {
        env.framework_state.avfoundation.av_player.live_players.remove(&this);
    }
}

- (NSInteger)actionAtItemEnd {
    env.objc.borrow::<AVPlayerHostObject>(this).action_at_item_end
}

- (())setActionAtItemEnd:(NSInteger)action {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).action_at_item_end = action;
}

- (bool)isExternalPlaybackActive {
    false
}

- (())seekToTime:(id)_time {
    // kCMTimeZero is the most common target; we ignore actual seeking
    // since there's no decoder.
}

- (())seekToTime:(id)_time toleranceBefore:(id)_toleranceBefore toleranceAfter:(id)_toleranceAfter {
    // As above.
}

- (())seekToTime:(id)_time
    toleranceBefore:(id)_toleranceBefore
    toleranceAfter:(id)_toleranceAfter
    completionHandler:(id)_handler {
    // For completion-handler variants (iOS 5+), the block should fire.
    // We don't yet have a generic block-invoker for AVFoundation callbacks,
    // so we silently ignore the handler. The async finish cycle will still
    // post DidPlayToEndTime, which is sufficient for most game intros.
}

@end

// ---------------------------------------------------------------------------
// AVPlayerLayer — extends CALayer
// ---------------------------------------------------------------------------
@implementation AVPlayerLayer: CALayer

+ (id)playerLayerWithPlayer:(id)player {
    let cls = env.objc.get_known_class("AVPlayerLayer", &mut env.mem);
    let alloc: id = msg![env; cls alloc];
    let init: id = msg![env; alloc init];
    () = msg![env; init setPlayer:player];
    autorelease(env, init)
}

- (id)init {
    let _: id = msg_super![env; this init];
    // Transparent by default; the game's OpenGL layer is behind us.
    () = msg![env; this setOpaque:false];
    let gravity = ns_string::get_static_str(
        env,
        crate::frameworks::avfoundation::av_capture::AVLayerVideoGravityResizeAspect,
    );
    player_layer_extras(env)
        .entry(this)
        .or_default()
        .video_gravity = gravity;
    this
}

- (())dealloc {
    let extra = player_layer_extras(env).remove(&this);
    if let Some(extra) = extra {
        if extra.player != nil {
            release(env, extra.player);
        }
        if extra.video_gravity != nil {
            release(env, extra.video_gravity);
        }
    }
    () = msg_super![env; this dealloc];
}

- (id)player {
    player_layer_extras(env)
        .get(&this)
        .map(|e| e.player)
        .unwrap_or(nil)
}

- (())setPlayer:(id)player {
    let old = {
        let entry = player_layer_extras(env).entry(this).or_default();
        let old = entry.player;
        entry.player = player;
        entry.ready_for_display = false;
        old
    };
    if old != nil {
        release(env, old);
    }
    if player != nil {
        retain(env, player);
    }
}

- (id)videoGravity {
    player_layer_extras(env)
        .get(&this)
        .map(|e| e.video_gravity)
        .unwrap_or(nil)
}

- (())setVideoGravity:(id)gravity {
    let old = {
        let entry = player_layer_extras(env).entry(this).or_default();
        let old = entry.video_gravity;
        entry.video_gravity = gravity;
        old
    };
    if old != nil {
        release(env, old);
    }
    if gravity != nil {
        retain(env, gravity);
    }
}

- (bool)readyForDisplay {
    player_layer_extras(env)
        .get(&this)
        .map(|e| e.ready_for_display)
        .unwrap_or(false)
}

- (CGRect)videoRect {
    // Return the layer's bounds — with no real decoder this is the best
    // approximation.
    let bounds: crate::frameworks::core_graphics::CGRect = msg![env; this bounds];
    bounds
}

@end

};
