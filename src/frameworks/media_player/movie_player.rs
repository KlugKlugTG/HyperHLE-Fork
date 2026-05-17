/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `MPMoviePlayerController` etc.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::{ns_string, ns_url, NSInteger};
use crate::frameworks::uikit::ui_device::UIDeviceOrientation;
use crate::media::{self, MovieBackend, PlayerEvent};
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, todo_objc_setter, ClassExports,
    HostObject, NSZonePtr,
};
use crate::Environment;
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Default)]
pub struct State {
    active_player: Option<id>,
    /// Various apps (e.g. Crash Bandicoot Nitro Kart 3D and Spore Origins)
    /// create or start a player and await some kind of notification, but can't
    /// handle it if that notification happens immediately.
    /// This queue lets us
    /// delay such notifications until the app next returns to the run loop,
    /// which seems to be late enough.
    pending_notifications: VecDeque<(&'static str, id, Instant)>,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.media_player.movie_player
    }
}

type MPMovieScalingMode = NSInteger;
type MPMovieControlStyle = NSInteger;
type MPMovieSourceType = NSInteger;
type MPMovieRepeatMode = NSInteger;

type MPMoviePlaybackState = NSInteger;
const MPMoviePlaybackStateStopped: MPMoviePlaybackState = 0;
const MPMoviePlaybackStatePlaying: MPMoviePlaybackState = 1;
const MPMoviePlaybackStatePaused: MPMoviePlaybackState = 2;

// Values might not be correct, but as these are linked symbol constants, it
// shouldn't matter.
pub const MPMoviePlayerPlaybackDidFinishNotification: &str =
    "MPMoviePlayerPlaybackDidFinishNotification";
/// Apparently an undocumented, private API. Spore Origins uses it.
pub const MPMoviePlayerContentPreloadDidFinishNotification: &str =
    "MPMoviePlayerContentPreloadDidFinishNotification";
pub const MPMoviePlayerScalingModeDidChangeNotification: &str =
    "MPMoviePlayerScalingModeDidChangeNotification";
pub const MPMoviePlayerPlaybackStateDidChangeNotification: &str =
    "MPMoviePlayerPlaybackStateDidChangeNotification";
pub const MPMoviePlayerLoadStateDidChangeNotification: &str =
    "MPMoviePlayerLoadStateDidChangeNotification";
pub const MPMoviePlayerNowPlayingMovieDidChangeNotification: &str =
    "MPMoviePlayerNowPlayingMovieDidChangeNotification";
pub const MPMoviePlayerWillEnterFullscreenNotification: &str =
    "MPMoviePlayerWillEnterFullscreenNotification";
pub const MPMoviePlayerDidEnterFullscreenNotification: &str =
    "MPMoviePlayerDidEnterFullscreenNotification";
pub const MPMoviePlayerWillExitFullscreenNotification: &str =
    "MPMoviePlayerWillExitFullscreenNotification";
pub const MPMoviePlayerDidExitFullscreenNotification: &str =
    "MPMoviePlayerDidExitFullscreenNotification";
// TODO: More notifications?
const MPMoviePlayerPlaybackDidFinishReasonUserInfoKey: &str =
    "MPMoviePlayerPlaybackDidFinishReasonUserInfoKey";

/// `NSNotificationName` values and other constants.
pub const CONSTANTS: ConstantExports = &[
    (
        "_MPMoviePlayerPlaybackDidFinishNotification",
        HostConstant::NSString(MPMoviePlayerPlaybackDidFinishNotification),
    ),
    (
        "_MPMoviePlayerContentPreloadDidFinishNotification",
        HostConstant::NSString(MPMoviePlayerContentPreloadDidFinishNotification),
    ),
    (
        "_MPMoviePlayerScalingModeDidChangeNotification",
        HostConstant::NSString(MPMoviePlayerScalingModeDidChangeNotification),
    ),
    (
        "_MPMoviePlayerPlaybackStateDidChangeNotification",
        HostConstant::NSString(MPMoviePlayerPlaybackStateDidChangeNotification),
    ),
    (
        "_MPMoviePlayerLoadStateDidChangeNotification",
        HostConstant::NSString(MPMoviePlayerLoadStateDidChangeNotification),
    ),
    (
        "_MPMoviePlayerNowPlayingMovieDidChangeNotification",
        HostConstant::NSString(MPMoviePlayerNowPlayingMovieDidChangeNotification),
    ),
    (
        "_MPMoviePlayerWillEnterFullscreenNotification",
        HostConstant::NSString(MPMoviePlayerWillEnterFullscreenNotification),
    ),
    (
        "_MPMoviePlayerDidEnterFullscreenNotification",
        HostConstant::NSString(MPMoviePlayerDidEnterFullscreenNotification),
    ),
    (
        "_MPMoviePlayerWillExitFullscreenNotification",
        HostConstant::NSString(MPMoviePlayerWillExitFullscreenNotification),
    ),
    (
        "_MPMoviePlayerDidExitFullscreenNotification",
        HostConstant::NSString(MPMoviePlayerDidExitFullscreenNotification),
    ),
    (
        "_MPMoviePlayerPlaybackDidFinishReasonUserInfoKey",
        HostConstant::NSString(MPMoviePlayerPlaybackDidFinishReasonUserInfoKey),
    ),
];

struct MPMoviePlayerControllerHostObject {
    // NSURL *
    content_url: id,
    // UIView *
    view: id,
    background_view: id,
    scaling_mode: MPMovieScalingMode,
    control_style: MPMovieControlStyle,
    source_type: MPMovieSourceType,
    repeat_mode: MPMovieRepeatMode,
    should_autoplay: bool,
    initial_playback_time: f64,
    playback_state: MPMoviePlaybackState,
    /// Real (or no-op) movie backend. Lifecycle is driven by play / pause /
    /// stop and by [`handle_players`]. The backend pulls decoded frames /
    /// audio off bounded queues and emits a [`PlayerEvent::Finished`] event
    /// when end-of-stream is reached, at which point we synthesize the same
    /// `MPMoviePlayerPlaybackDidFinishNotification` the existing stub
    /// produced. When `--feature ffmpeg` is enabled this is a real FFmpeg
    /// pipeline; otherwise it is a `NullBackend` that mirrors the legacy
    /// "fake-finish-after-150ms" behaviour.
    backend: Option<Box<dyn MovieBackend + Send>>,
    /// Set once we have already emitted the finish notification via the
    /// backend driver, so we don't double-fire when the existing
    /// `pending_notifications` queue also reaches the same player.
    backend_finish_emitted: bool,
}
impl HostObject for MPMoviePlayerControllerHostObject {}

/// Ensure the player has a valid dummy view, creating one lazily if needed.
/// Returns the view id (always non-nil after this call).
/// Materialize the guest movie file referenced by `url` to a host-side
/// temporary file and open it with the best available [`MovieBackend`].
///
/// Returns `None` if anything along the way fails – the caller falls back
/// to the historical "fake-finish-after-150ms" behaviour, so missing /
/// undecodable videos behave exactly like they used to.
///
/// In default builds (no `ffmpeg` feature) this is a cheap no-op that
/// returns `None`, so the legacy stub path is preserved bit-for-bit.
fn try_open_backend(env: &mut Environment, url: id) -> Option<Box<dyn MovieBackend + Send>> {
    if cfg!(not(feature = "ffmpeg")) {
        return None;
    }

    use std::io::Write;

    let guest_path = ns_url::to_rust_path(env, url);
    let bytes = match env.fs.read(&*guest_path) {
        Ok(b) => b,
        Err(()) => {
            log!(
                "MPMoviePlayerController: could not read guest file {:?}; \
                 falling back to stub backend.",
                guest_path
            );
            return None;
        }
    };

    // Use the file_name only as a hint so FFmpeg's format probing keeps
    // working (some demuxers sniff the extension). Place under a stable
    // host directory rather than std::env::temp_dir() so we can clean up
    // on emulator restart.
    let file_name = guest_path
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or("movie.bin");
    let host_dir = crate::paths::user_data_base_path().join("touchHLE_movie_tmp");
    if let Err(e) = std::fs::create_dir_all(&host_dir) {
        log!(
            "MPMoviePlayerController: could not create temp dir {:?}: {}; \
             falling back to stub backend.",
            host_dir,
            e
        );
        return None;
    }
    let host_path = host_dir.join(file_name);

    match std::fs::File::create(&host_path).and_then(|mut f| f.write_all(&bytes)) {
        Ok(()) => {}
        Err(e) => {
            log!(
                "MPMoviePlayerController: could not stage movie to {:?}: {}; \
                 falling back to stub backend.",
                host_path,
                e
            );
            return None;
        }
    }

    let backend = media::open_movie(&host_path);
    if backend.video_config().is_none() && backend.audio_config().is_none() {
        // The NullBackend is fine, but log once so it's clear we are in
        // fallback mode.
        log!(
            "MPMoviePlayerController: no real movie backend available for \
             {:?}; playback will be stubbed.",
            guest_path
        );
    }
    Some(backend)
}

fn ensure_view(env: &mut Environment, this: id) -> id {
    let existing = env
        .objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .view;
    if existing != nil {
        return existing;
    }
    let view_alloc: id = msg_class![env; UIView alloc];
    let view: id = msg![env; view_alloc init];
    retain(env, view);
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .view = view;
    view
}

fn ensure_background_view(env: &mut Environment, this: id) -> id {
    let existing = env
        .objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .background_view;
    if existing != nil {
        return existing;
    }
    let view_alloc: id = msg_class![env; UIView alloc];
    let view: id = msg![env; view_alloc init];
    retain(env, view);
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .background_view = view;
    view
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation MPMoviePlayerController: NSObject

// TODO: actual playback

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(MPMoviePlayerControllerHostObject {
        content_url: nil,
        view: nil,
        background_view: nil,
        scaling_mode: 0,
        control_style: 0,
        source_type: 0,
        repeat_mode: 0,
        should_autoplay: true,

        initial_playback_time: -1.0,
        playback_state: MPMoviePlaybackStateStopped,
        backend: None,
        backend_finish_emitted: false,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithContentURL:(id)url { // NSURL*
    log!(
        "TODO: [(MPMoviePlayerController*){:?} initWithContentURL:{:?} ({:?})]",
        this,
        url,
        ns_url::to_rust_path(env, url),
    );

    // Инициализируем сам объект
    let this: id = msg![env; this init];
    retain(env, url);

    {
        let mut host = env.objc.borrow_mut::<MPMoviePlayerControllerHostObject>(this);
        host.content_url = url;
    }

    // Ensure views exist immediately
    ensure_view(env, this);
    ensure_background_view(env, this);

    // Try to open a real backend for the supplied URL. If this fails for
    // any reason (missing file, unsupported format, FFmpeg disabled) we
    // fall back to the legacy "fake finish after 150ms" stub so the host
    // game can still progress past the cut-scene.
    let backend = try_open_backend(env, url);
    let backend_opened_real_stream = backend
        .as_ref()
        .map(|b| b.video_config().is_some() || b.audio_config().is_some())
        .unwrap_or(false);
    {
        let mut host = env.objc.borrow_mut::<MPMoviePlayerControllerHostObject>(this);
        host.backend = backend;
        host.backend_finish_emitted = false;
    }

    // Act as if loading immediately completed (Spore Origins waits for this).
    // Retain this so the object stays alive until handle_players fires.
    retain(env, this);
    State::get(env).pending_notifications.push_back((
        MPMoviePlayerContentPreloadDidFinishNotification,
        this,
        Instant::now(),
    ));

    if !backend_opened_real_stream {
        // ХАК ДЛЯ ЗАГЛУШКИ: автоматически завершаем «видео» через 150мс,
        // только если реальный backend недоступен. С реальным backend
        // финиш-нотификацию сгенерирует [`handle_players`] по событию
        // `PlayerEvent::Finished`.
        retain(env, this);
        State::get(env).pending_notifications.push_back((
            MPMoviePlayerPlaybackDidFinishNotification,
            this,
            Instant::now() + std::time::Duration::from_millis(150),
        ));
    }

    this
}

- (())dealloc {
    let url = env
        .objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .content_url;
    release(env, url);

    let view = env
        .objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .view;
    release(env, view);

    let bg_view = env
        .objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .background_view;
    release(env, bg_view);

    // Stop and drop the backend before deallocating the host object so its
    // worker thread joins cleanly.
    if let Some(mut backend) = env
        .objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .backend
        .take()
    {
        backend.stop();
        drop(backend);
    }

    env.objc.dealloc_object(this, &mut env.mem);
}

- (id)contentURL {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .content_url
}

- (id)backgroundColor {
    msg_class![env;
UIColor blackColor] // TODO
}
- (())setBackgroundColor:(id)color { // UIColor*
    todo_objc_setter!(this, color);
}

// --- Scaling mode ---

- (MPMovieScalingMode)scalingMode {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .scaling_mode
}
- (())setScalingMode:(MPMovieScalingMode)mode {
    log!(
        "TODO: [(MPMoviePlayerController*){:?} setScalingMode:{:?}]",
        this,
        mode
    );
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .scaling_mode = mode;
}

// --- Control style ---

- (MPMovieControlStyle)controlStyle {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .control_style
}
- (())setControlStyle:(MPMovieControlStyle)style {
    log!(
        "TODO: [(MPMoviePlayerController*){:?} setControlStyle:{:?}]",
        this,
        style
    );
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .control_style = style;
}

// --- Source type ---

- (MPMovieSourceType)movieSourceType {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .source_type
}
- (())setMovieSourceType:(MPMovieSourceType)source_type {
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .source_type = source_type;
}

// --- Repeat mode ---

- (MPMovieRepeatMode)repeatMode {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .repeat_mode
}
- (())setRepeatMode:(MPMovieRepeatMode)mode {
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .repeat_mode = mode;
}

// --- Autoplay ---

- (bool)shouldAutoplay {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .should_autoplay
}
- (())setShouldAutoplay:(bool)autoplay {
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .should_autoplay = autoplay;
}

// --- Misc setters ---

- (())setUseApplicationAudioSession:(bool)use_session {
    todo_objc_setter!(this, use_session);
}

- (())setFullscreen:(bool)fullscreen {
    todo_objc_setter!(this, fullscreen);
}

- (())setFullscreen:(bool)fullscreen animated:(bool)animated {
    log!(
        "TODO: [(MPMoviePlayerController*){:?} setFullscreen:{:?} animated:{:?}]",
        this,
        fullscreen,
        animated
    );
}

// --- View ---

// Returns the player's backing view. Created lazily if initWithContentURL:
// somehow failed to allocate it, so this always returns a non-nil UIView.
- (id)view {
    ensure_view(env, this)
}

- (id)backgroundView {
    ensure_background_view(env, this)
}
- (())setBackgroundView:(id)view {
    todo_objc_setter!(this, view);
}

// --- Playback state / time ---

- (MPMoviePlaybackState)playbackState {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .playback_state
}

- (f64)currentPlaybackTime {
    1.0 // Return non-zero dummy time
}
- (())setCurrentPlaybackTime:(f64)time {
    todo_objc_setter!(this, time);
}

- (f64)initialPlaybackTime {
    env.objc
        .borrow::<MPMoviePlayerControllerHostObject>(this)
        .initial_playback_time
}
- (())setInitialPlaybackTime:(f64)time {
    env.objc
        .borrow_mut::<MPMoviePlayerControllerHostObject>(this)
        .initial_playback_time = time;
}

- (f64)duration {
    1.0 // Return non-zero to prevent division by zero in game engines
}
- (f64)playableDuration {
    1.0
}
- (bool)isPreparedToPlay {
    true
}
- (bool)readyForDisplay {
    true
}

- (())prepareToPlay {
    // Act as if we are immediately prepared;
    // no real playback yet.
}

// Apparently an undocumented, private API, but Spore Origins uses it.
- (())setMovieControlMode:(NSInteger)_mode {
    // As this is undocumented and we don't have real video playback yet, let's
    // ignore it.
}

// Another undocumented one! But some apps may still use it :/
// https://stackoverflow.com/a/1390079/2241008
- (())setOrientation:(UIDeviceOrientation)_orientation animated:(bool)_animated {
}

// MPMediaPlayback implementation
- (())play {
    log!("[(MPMoviePlayerController*){:?} play]", this);
    let has_real_backend = {
        let mut host = env.objc.borrow_mut::<MPMoviePlayerControllerHostObject>(this);
        host.playback_state = MPMoviePlaybackStatePlaying;
        match host.backend.as_mut() {
            Some(b) => {
                b.play();
                b.video_config().is_some() || b.audio_config().is_some()
            }
            None => false,
        }
    };

    if !has_real_backend {
        // Legacy stub: synthesise the finish notification on a short timer
        // so games waiting for it can proceed past the cut-scene.
        retain(env, this);
        State::get(env).pending_notifications.push_back((
            MPMoviePlayerPlaybackDidFinishNotification,
            this,
            Instant::now() + std::time::Duration::from_millis(50),
        ));
    } else {
        env.framework_state
            .media_player
            .movie_player
            .active_player = Some(this);
        retain(env, this);
    }
}

- (())pause {
    log!("[(MPMoviePlayerController*){:?} pause]", this);
    let mut host = env.objc.borrow_mut::<MPMoviePlayerControllerHostObject>(this);
    host.playback_state = MPMoviePlaybackStatePaused;
    if let Some(b) = host.backend.as_mut() {
        b.pause();
    }
}

- (())stop {
    log!("[(MPMoviePlayerController*){:?} stop]", this);
    {
        let mut host = env.objc.borrow_mut::<MPMoviePlayerControllerHostObject>(this);
        host.playback_state = MPMoviePlaybackStateStopped;
        if let Some(b) = host.backend.as_mut() {
            b.stop();
        }
    }
    if env
        .framework_state
        .media_player
        .movie_player
        .active_player == Some(this)
    {
        env.framework_state.media_player.movie_player.active_player = None;
        release(env, this);
    }
}

@end

@implementation MPMoviePlayerViewController: UIViewController

- (id)initWithContentURL:(id)url {
    log!(
        "TODO: [(MPMoviePlayerViewController*){:?} initWithContentURL:{:?} ({:?})]",
        this,
        url,
        ns_url::to_rust_path(env, url),
    );
    // Call designated initializer of UIViewController superclass
    let this: id = msg![env; this init];
    this
}

@end

};
/// For use by `NSRunLoop` via [super::handle_players]: check movie players'
/// status, send notifications if necessary.
pub(super) fn handle_players(env: &mut Environment) {
    // First, drive any active real backend forward: drain its event channel,
    // its video queue (frames are dropped for now — rendering is handled in
    // a follow-up step) and its audio queue (samples are also dropped
    // pending a wiring into the OpenAL output). When the backend reports
    // `Finished` (or its worker has died), we synthesise the same
    // `MPMoviePlayerPlaybackDidFinishNotification` the legacy stub emitted.
    let active_player = env.framework_state.media_player.movie_player.active_player;
    if let Some(player) = active_player {
        let mut emit_finish = false;
        let mut emit_failed: Option<String> = None;
        {
            let host = env
                .objc
                .borrow_mut::<MPMoviePlayerControllerHostObject>(player);
            if !host.backend_finish_emitted {
                if let Some(backend) = host.backend.as_mut() {
                    // Drain pending decoded data – discarding for now keeps
                    // the queues from filling up so the decoder can make
                    // progress.
                    while backend.try_next_video_frame().is_some() {}
                    while backend.try_next_audio_chunk().is_some() {}
                    while let Some(event) = backend.try_recv_event() {
                        match event {
                            PlayerEvent::Started => {}
                            PlayerEvent::Finished => {
                                emit_finish = true;
                            }
                            PlayerEvent::Failed(msg) => {
                                emit_failed = Some(msg);
                            }
                        }
                    }
                }
            }
            if emit_finish || emit_failed.is_some() {
                host.backend_finish_emitted = true;
                host.playback_state = MPMoviePlaybackStateStopped;
            }
        }
        if let Some(msg) = emit_failed.clone() {
            log!(
                "MPMoviePlayerController: backend reported failure: {}; \
                 emitting PlaybackDidFinish anyway.",
                msg
            );
        }
        if emit_finish || emit_failed.is_some() {
            // Don't double-queue the finish notification if something else
            // (e.g. the legacy stub or an explicit `stop` call) already
            // scheduled it.
            let already_queued =
                State::get(env)
                    .pending_notifications
                    .iter()
                    .any(|(name, obj, _)| {
                        *name == MPMoviePlayerPlaybackDidFinishNotification && *obj == player
                    });
            if !already_queued {
                retain(env, player);
                State::get(env).pending_notifications.push_back((
                    MPMoviePlayerPlaybackDidFinishNotification,
                    player,
                    Instant::now(),
                ));
            }
        }
    }

    let mut notifs_to_run = Vec::new();
    let pending_notifs = &mut State::get(env).pending_notifications;
    let mut i = 0;
    while i < pending_notifs.len() {
        let (name_str, object, time) = pending_notifs[i];
        if Instant::now() >= time {
            notifs_to_run.push((name_str, object));
            pending_notifs.swap_remove_back(i);
        } else {
            i += 1;
        }
    }

    for (name_str, object) in notifs_to_run {
        // Update playback state before posting so that any handler which
        // checks [player playbackState] sees Stopped immediately.
        if name_str == MPMoviePlayerPlaybackDidFinishNotification {
            env.objc
                .borrow_mut::<MPMoviePlayerControllerHostObject>(object)
                .playback_state = MPMoviePlaybackStateStopped;
        }

        let name = ns_string::get_static_str(env, name_str);
        let center: id = msg_class![env; NSNotificationCenter defaultCenter];
        if name_str == MPMoviePlayerPlaybackDidFinishNotification {
            // Many apps (including NFSU) read
            // MPMoviePlayerPlaybackDidFinishReasonUserInfoKey from the
            // notification's userInfo.
            // Without it the game dereferences nil
            // at offset 0x10, causing a NULL-PAGE READ crash.
            // MPMovieFinishReasonPlaybackEnded = 0
            let reason_num: id = msg_class![env;
NSNumber numberWithInt:0i32];
            let reason_key =
                ns_string::get_static_str(env, MPMoviePlayerPlaybackDidFinishReasonUserInfoKey);
            let user_info: id = msg_class![env; NSDictionary
                dictionaryWithObject:reason_num
                forKey:reason_key];
            let _: () = msg![env; center postNotificationName:name
                                                       object:object

              userInfo:user_info];
        } else {
            let _: () = msg![env;
center postNotificationName:name
                                                       object:object];
        }

        // Release the retain we took when queuing this notification.
        release(env, object);
    }
}
