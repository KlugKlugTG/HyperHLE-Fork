/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIWebView`.

use crate::android_web_view;
use crate::frameworks::core_graphics::{cg_image, CGRect};
use crate::frameworks::foundation::ns_string::{self, to_rust_string};
use crate::frameworks::foundation::NSUInteger;
use crate::frameworks::uikit::ui_view::UIViewHostObject;
use crate::image::Image;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, nil, objc_classes, release, retain,
    ClassExports, HostObject, NSZonePtr,
};
use crate::Environment;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// UIWebViewNavigationType constants
pub type UIWebViewNavigationType = i32;
pub const UIWebViewNavigationTypeLinkClicked: UIWebViewNavigationType = 0;
pub const UIWebViewNavigationTypeFormSubmitted: UIWebViewNavigationType = 1;
pub const UIWebViewNavigationTypeBackForward: UIWebViewNavigationType = 2;
pub const UIWebViewNavigationTypeReload: UIWebViewNavigationType = 3;
pub const UIWebViewNavigationTypeFormResubmitted: UIWebViewNavigationType = 4;
pub const UIWebViewNavigationTypeOther: UIWebViewNavigationType = 5;

// UIDataDetectorTypes bitmask
pub type UIDataDetectorTypes = NSUInteger;
pub const UIDataDetectorTypePhoneNumber: UIDataDetectorTypes = 1 << 0;
pub const UIDataDetectorTypeLink: UIDataDetectorTypes = 1 << 1;
pub const UIDataDetectorTypeAddress: UIDataDetectorTypes = 1 << 2;
pub const UIDataDetectorTypeCalendarEvent: UIDataDetectorTypes = 1 << 3;
pub const UIDataDetectorTypeNone: UIDataDetectorTypes = 0;
pub const UIDataDetectorTypeAll: UIDataDetectorTypes = u32::MAX as UIDataDetectorTypes;

#[derive(Default)]
struct UIWebViewHostObject {
    superclass: UIViewHostObject,
    /// UIWebViewDelegate — weak reference (no retain per Apple docs)
    delegate: id,
    scales_page_to_fit: bool,
    detects_phone_numbers: bool,
    data_detector_types: UIDataDetectorTypes,
    allows_inline_media_playback: bool,
    media_playback_requires_user_action: bool,
    media_playback_allows_air_play: bool,
    suppress_incremental_rendering: bool,
    keyboard_display_requires_user_action: bool,
    pagination_mode: i32,
    pagination_breaking_mode: i32,
    page_length: f64,
    gap_between_pages: f64,
    /// NSString* — last URL string passed to loadRequest:
    current_url: id,

    loading: bool,
    /// Simple back/forward stack — NSString* items.
    back_stack: Vec<id>,
    forward_stack: Vec<id>,
    /// Host-side native overlay (Android WebView) id. `-2` = no overlay has
    /// been created yet (first `show` seeds it with a page); `-1` = a show
    /// was attempted but unavailable/zero-size; `>= 0` = live overlay id.
    overlay_id: i32,
}
impl_HostObject_with_superclass!(UIWebViewHostObject);

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIWebView: UIView

// =========================================================================
// MARK: - Allocation
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIWebViewHostObject {
        superclass: UIViewHostObject::default(),
        delegate: nil,
        scales_page_to_fit: false,
        detects_phone_numbers: true,
        data_detector_types: UIDataDetectorTypePhoneNumber,
        allows_inline_media_playback: false,
        media_playback_requires_user_action: true,
        media_playback_allows_air_play: true,
        suppress_incremental_rendering: false,
        keyboard_display_requires_user_action: true,

        pagination_mode: 0,
        pagination_breaking_mode: 0,
        page_length: 0.0,
        gap_between_pages: 0.0,
        current_url: nil,
        loading: false,
        back_stack: Vec::new(),
        forward_stack: Vec::new(),
        overlay_id: -2,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// =========================================================================
// MARK: - Initializers
// =========================================================================

- (id)init {
    this
}

- (id)initWithFrame:(CGRect)_frame {
    this
}

- (id)initWithCoder:(id)_coder {
    this
}

// =========================================================================
// MARK: - Dealloc
// =========================================================================

- (())dealloc {
    let host = env.objc.borrow::<UIWebViewHostObject>(this);
    let (current_url, back_stack, forward_stack, overlay_id) = (
        host.current_url,
        host.back_stack.clone(),
        host.forward_stack.clone(),
        host.overlay_id,
    );
    if overlay_id >= 0 {
        android_web_view::hide(overlay_id);
    }
    release(env, current_url);
    for url in back_stack    { release(env, url); }
    for url in forward_stack { release(env, url); }
    env.objc.dealloc_object(this, &mut env.mem)
}

// =========================================================================
// MARK: - Delegate
// =========================================================================

- (id)delegate {
    env.objc.borrow::<UIWebViewHostObject>(this).delegate
}

- (())setDelegate:(id)delegate {
    // Weak reference — do NOT retain.
    env.objc.borrow_mut::<UIWebViewHostObject>(this).delegate = delegate;
}

// =========================================================================
// MARK: - Loading
// =========================================================================

- (())loadRequest:(id)request { // NSURLRequest*
    let url_string: String = if request != nil {
        let url: id = msg![env; request URL];
        let url_desc: id = msg![env; url description];
        if url_desc != nil { to_rust_string(env, url_desc).into_owned() } else { String::new() }
    } else {
        String::new()
    };
    log!("UIWebView loadRequest: {}", url_string);

    // Push current URL onto back stack before navigating.
    let old_url = env.objc.borrow::<UIWebViewHostObject>(this).current_url;
    if old_url != nil {
        retain(env, old_url);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).back_stack.push(old_url);
        // Clear forward stack on new navigation.
        let fwd: Vec<id> = std::mem::take(
            &mut env.objc.borrow_mut::<UIWebViewHostObject>(this).forward_stack
        );
        for u in fwd { release(env, u); }
    }
    release(env, old_url);
    let ns_url = ns_string::from_rust_string(env, url_string.clone());
    {
        let host = env.objc.borrow_mut::<UIWebViewHostObject>(this);
        host.current_url = ns_url;
        host.loading = true;
    }
    fire_did_start_load(env, this);

    // Real engine path: hand the request to a genuine Android WebView
    // overlay; did-finish is scheduled for after the native page loads.
    if android_web_view::native_webview_available()
        && overlay_load(env, this, Some(&url_string), None)
    {
        schedule_did_finish_load(env, this);
        return;
    }

    // Desktop fallback: snapshot the URL with headless Chromium and install
    // the PNG as this view's layer contents. We don't stream content, so the
    // load is "finished" as soon as the snapshot is in.
    let frame: CGRect = msg![env; this frame];
    render_url_to_layer(env, this, &url_string, frame);
    finish_load(env, this);
}

- (())loadHTMLString:(id)html     // NSString*
            baseURL:(id)_base_url { // NSURL*
    let html_str = if html != nil {
        to_rust_string(env, html).into_owned()
    } else { String::new() };
    log_dbg!("UIWebView loadHTMLString: ({} chars)", html_str.len());

    if android_web_view::native_webview_available()
        && overlay_load(env, this, None, Some((&html_str, "text/html")))
    {
        schedule_did_finish_load(env, this);
        return;
    }

    // Desktop fallback: write the HTML to a temp file and snapshot it.
    if !html_str.is_empty() {
        let idx = SNAP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("touchhle_uiwebview_{}.html", idx));
        if std::fs::write(&tmp, html_str.as_bytes()).is_ok() {
            let url = format!("file://{}", tmp.display());
            let frame: CGRect = msg![env; this frame];
            render_url_to_layer(env, this, &url, frame);
            let _ = std::fs::remove_file(&tmp);
        }
    }
    finish_load(env, this);
}

- (())loadData:(id)data            // NSData*
      MIMEType:(id)mime            // NSString*
      textEncodingName:(id)_enc    // NSString*
       baseURL:(id)_base_url {     // NSURL*
    let mime_str = if mime != nil { to_rust_string(env, mime).into_owned() } else { "(null)".into() };
    log_dbg!("UIWebView loadData:MIMEType:{}", mime_str);

    // Read the payload out of the guest NSData.
    let payload = if data != nil {
        let len: NSUInteger = msg![env; data length];
        let bytes_ptr: crate::mem::ConstPtr<u8> = msg![env; data bytes];
        if len > 0 && !bytes_ptr.is_null() {
            let bytes = env.mem.bytes_at(bytes_ptr, len);
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if android_web_view::native_webview_available()
        && overlay_load(env, this, None, Some((&payload, mime_str.as_str())))
    {
        schedule_did_finish_load(env, this);
        return;
    }

    // Desktop fallback: only HTML payloads can be rendered (temp file route).
    if mime_str.starts_with("text/html") && !payload.is_empty() {
        let idx = SNAP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("touchhle_uiwebview_{}.html", idx));
        if std::fs::write(&tmp, payload.as_bytes()).is_ok() {
            let url = format!("file://{}", tmp.display());
            let frame: CGRect = msg![env; this frame];
            render_url_to_layer(env, this, &url, frame);
            let _ = std::fs::remove_file(&tmp);
        }
    }
    finish_load(env, this);
}

// =========================================================================
// MARK: - Subview management
// =========================================================================

- (())insertSubview:(id)view aboveSubview:(id)sibling {
    if view == nil { return; }

    // If sibling is nil or not in our view hierarchy, just add at the top.
    if sibling == nil {
        let _: () = msg![env; this addSubview:view];
        return;
    }

    // Delegate to UIView's insertSubview:aboveSubview: on our own view.
    let self_view: id = msg![env; this view];
    if self_view != nil {
        let _: () = msg![env; self_view insertSubview:view aboveSubview:sibling];
    } else {
        // Fallback — just add it.
        let _: () = msg![env; this addSubview:view];
    }
}

- (())reload {
    log_dbg!("UIWebView reload");
    let current = env.objc.borrow::<UIWebViewHostObject>(this).current_url;
    if current == nil { return; }
    retain(env, current);

    // Вынесено в отдельную переменную для предотвращения ошибки E0283
    let url: id = msg_class![env; NSURL URLWithString:current];
    let ns_req: id = msg_class![env; NSURLRequest requestWithURL:url];

    let _: () = msg![env; this loadRequest:ns_req];
    release(env, current);
}

- (())stopLoading {
    log_dbg!("UIWebView stopLoading");
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 {
        android_web_view::stop_loading(overlay_id);
    }
    env.objc.borrow_mut::<UIWebViewHostObject>(this).loading = false;

    let delegate = env.objc.borrow::<UIWebViewHostObject>(this).delegate;
    if delegate != nil {
        let sel = env.objc.register_host_selector(
            "webView:didFailLoadWithError:".to_string(),
            &mut env.mem,
        );
        let responds: bool = msg![env; delegate respondsToSelector:sel];
        if responds {
            let error: id = nil;
            let _: () = msg![env; delegate webView:this didFailLoadWithError:error];
        }
    }
}

- (bool)isLoading {
    env.objc.borrow::<UIWebViewHostObject>(this).loading
}

// =========================================================================
// MARK: - Navigation
// =========================================================================

- (bool)canGoBack {
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 && android_web_view::can_go_back(overlay_id) {
        return true;
    }
    !env.objc.borrow::<UIWebViewHostObject>(this).back_stack.is_empty()
}

- (bool)canGoForward {
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 && android_web_view::can_go_forward(overlay_id) {
        return true;
    }
    !env.objc.borrow::<UIWebViewHostObject>(this).forward_stack.is_empty()
}

- (())goBack {
    let back_url = {
        let host = env.objc.borrow_mut::<UIWebViewHostObject>(this);
        host.back_stack.pop()
    };
    let Some(back_url) = back_url else { return; };

    // Push current to forward stack.
    let current = env.objc.borrow::<UIWebViewHostObject>(this).current_url;
    if current != nil {
        retain(env, current);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).forward_stack.push(current);
    }
    release(env, current);

    // Android: the native WebView keeps its own history; just drive it.
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 && android_web_view::native_webview_available() {
        android_web_view::go_back(overlay_id);
    } else if android_web_view::native_webview_available() {
        let url_str = to_rust_string(env, back_url).into_owned();
        overlay_load(env, this, Some(&url_str), None);
    }
    env.objc.borrow_mut::<UIWebViewHostObject>(this).current_url = back_url;
    env.objc.borrow_mut::<UIWebViewHostObject>(this).loading = false;
    finish_load(env, this);
}

- (())goForward {
    let fwd_url = {
        let host = env.objc.borrow_mut::<UIWebViewHostObject>(this);
        host.forward_stack.pop()
    };
    let Some(fwd_url) = fwd_url else { return; };

    let current = env.objc.borrow::<UIWebViewHostObject>(this).current_url;
    if current != nil {
        retain(env, current);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).back_stack.push(current);
    }
    release(env, current);

    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 && android_web_view::native_webview_available() {
        android_web_view::go_forward(overlay_id);
    } else if android_web_view::native_webview_available() {
        let url_str = to_rust_string(env, fwd_url).into_owned();
        overlay_load(env, this, Some(&url_str), None);
    }
    env.objc.borrow_mut::<UIWebViewHostObject>(this).current_url = fwd_url;
    env.objc.borrow_mut::<UIWebViewHostObject>(this).loading = false;
    finish_load(env, this);
}

// =========================================================================
// MARK: - JavaScript
// =========================================================================

- (id)stringByEvaluatingJavaScriptFromString:(id)script { // NSString* -> NSString*
    let script_str = if script != nil {
        to_rust_string(env, script).into_owned()
    } else { String::new() };
    log_dbg!("UIWebView stringByEvaluatingJavaScriptFromString: {:?}", script_str);

    // Real path: evaluate in the native Android WebView and return its
    // result. Android returns the value JSON-encoded; unwrap plain strings
    // and nulls so guests see something close to UIWebView's conversion.
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 {
        if let Some(result) = android_web_view::eval_js(overlay_id, &script_str) {
            let unwrapped = unwrap_js_result(&result);
            let s = ns_string::from_rust_string(env, unwrapped);
            return crate::objc::autorelease(env, s);
        }
    }
    // Return empty NSString rather than nil — some apps check the return value.
    let empty = ns_string::from_rust_string(env, String::new());
    crate::objc::autorelease(env, empty)
}

// =========================================================================
// MARK: - Request / URL accessors
// =========================================================================

- (id)request { // NSURLRequest*
    let url = env.objc.borrow::<UIWebViewHostObject>(this).current_url;
    if url == nil {
        return nil;
    }
    let ns_url: id = msg_class![env; NSURL URLWithString:url];
    if ns_url == nil {
        return nil;
    }
    msg_class![env; NSURLRequest requestWithURL:ns_url]
}

// Returns the URL of the currently loaded page as an NSString*.
- (id)_currentURLString { // NSString* (private helper)
    env.objc.borrow::<UIWebViewHostObject>(this).current_url
}

// =========================================================================
// MARK: - Properties
// =========================================================================

- (bool)scalesPageToFit {
    env.objc.borrow::<UIWebViewHostObject>(this).scales_page_to_fit
}
- (())setScalesPageToFit:(bool)scales {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).scales_page_to_fit = scales;
}

- (bool)detectsPhoneNumbers {
    env.objc.borrow::<UIWebViewHostObject>(this).detects_phone_numbers
}
- (())setDetectsPhoneNumbers:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).detects_phone_numbers = value;
}

- (UIDataDetectorTypes)dataDetectorTypes {
    env.objc.borrow::<UIWebViewHostObject>(this).data_detector_types
}
- (())setDataDetectorTypes:(UIDataDetectorTypes)types {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).data_detector_types = types;
}

- (bool)allowsInlineMediaPlayback {
    env.objc.borrow::<UIWebViewHostObject>(this).allows_inline_media_playback
}
- (())setAllowsInlineMediaPlayback:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).allows_inline_media_playback = value;
}

- (bool)mediaPlaybackRequiresUserAction {
    env.objc.borrow::<UIWebViewHostObject>(this).media_playback_requires_user_action
}
- (())setMediaPlaybackRequiresUserAction:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).media_playback_requires_user_action = value;
}

- (bool)mediaPlaybackAllowsAirPlay {
    env.objc.borrow::<UIWebViewHostObject>(this).media_playback_allows_air_play
}
- (())setMediaPlaybackAllowsAirPlay:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).media_playback_allows_air_play = value;
}

- (bool)suppressesIncrementalRendering {
    env.objc.borrow::<UIWebViewHostObject>(this).suppress_incremental_rendering
}
- (())setSuppressesIncrementalRendering:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).suppress_incremental_rendering = value;
}

- (bool)keyboardDisplayRequiresUserAction {
    env.objc.borrow::<UIWebViewHostObject>(this).keyboard_display_requires_user_action
}
- (())setKeyboardDisplayRequiresUserAction:(bool)value {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).keyboard_display_requires_user_action = value;
}

// Pagination (iOS 7+)
- (i32)paginationMode { // UIWebPaginationMode
    env.objc.borrow::<UIWebViewHostObject>(this).pagination_mode
}
- (())setPaginationMode:(i32)mode {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).pagination_mode = mode;
}

- (i32)paginationBreakingMode { // UIWebPaginationBreakingMode
    env.objc.borrow::<UIWebViewHostObject>(this).pagination_breaking_mode
}
- (())setPaginationBreakingMode:(i32)mode {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).pagination_breaking_mode = mode;
}

- (f64)pageLength {
    env.objc.borrow::<UIWebViewHostObject>(this).page_length
}
- (())setPageLength:(f64)length {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).page_length = length;
}

- (f64)gapBetweenPages {
    env.objc.borrow::<UIWebViewHostObject>(this).gap_between_pages
}
- (())setGapBetweenPages:(f64)gap {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).gap_between_pages = gap;
}

- (u32)pageCount { // NSUInteger
    // No real rendering — always 0.
    0u32
}

// =========================================================================
// MARK: - View hierarchy (overlay teardown)
// =========================================================================

- (())removeFromSuperview {
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 {
        android_web_view::hide(overlay_id);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).overlay_id = -1;
    }
    // Mirror UIView's removeFromSuperview: detach from the superview. The
    // layer detaches itself from the superlayer; the superview drops `this`
    // from its `subviews` (which balances the retain taken by addSubview).
    let superview = env
        .objc
        .borrow_mut::<UIWebViewHostObject>(this)
        .superclass
        .superview;
    if superview == nil {
        return;
    }
    let layer = env.objc.borrow::<UIWebViewHostObject>(this).superclass.layer;
    let _: () = msg![env; layer removeFromSuperlayer];
    let removed = {
        let subviews = &mut env
            .objc
            .borrow_mut::<UIViewHostObject>(superview)
            .subviews;
        if let Some(idx) = subviews.iter().position(|&v| v == this) {
            subviews.remove(idx);
            true
        } else {
            false
        }
    };
    if removed {
        release(env, this);
    }
}

// =========================================================================
// MARK: - Scroll view
// =========================================================================

// Returns a stub scroll view so apps that access scrollView don't crash.
- (id)scrollView { // UIScrollView*
    // Return self as a passthrough — we don't have a real UIScrollView here.
    this
}

// =========================================================================
// MARK: - Native-overlay & async-load internals
// =========================================================================

// Called by an NSTimer scheduled in loadRequest:/loadHTMLString:/loadData:.
// The timer argument is the (retained) NSTimer; we release our retain of
// `this` here to keep refcounts balanced.
- (())touchhleWebViewLoadDidFinish:(id)_timer {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).loading = false;
    fire_did_finish_load(env, this);
    release(env, this);
}



// =========================================================================
// MARK: - Description
// =========================================================================

- (id)description {
    let (loading, current_url) = {
        let h = env.objc.borrow::<UIWebViewHostObject>(this);
        (h.loading, h.current_url)
    };
    let url_str = if current_url != nil {
        to_rust_string(env, current_url).into_owned()
    } else { "(nil)".into() };
    let s = format!(
        "<UIWebView: {:?}; loading={}; url={}>",
        this, loading, url_str
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};

// =========================================================================
// MARK: - Chromium/CDP bridge: render a URL into the view's layer.contents
// =========================================================================
//
// touchHLE has no HTML rendering engine. As an opportunistic fallback (see
// PR description) we shell out to the host's headless Chromium to rasterise
// the target URL into a PNG, then install that PNG as the CALayer contents
// for the UIWebView. This gives apps like Google Mobile a visible web page
// instead of a blank rectangle, at the cost of interactivity.

/// Counter used for unique temp filenames.
static SNAP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Find a headless-capable Chromium binary on the host. Returns `None` when
/// no suitable browser is available — in that case we leave the layer blank.
fn find_chromium_binary() -> Option<PathBuf> {
    // Allow env var override for advanced users / CI.
    if let Ok(path) = std::env::var("TOUCHHLE_CHROMIUM") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let candidates = [
        "/opt/.devin/chrome/chrome/linux-137.0.7118.2/chrome-linux64/chrome",
        "/opt/.devin/playwright_browsers/chromium-1097/chrome-linux/chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome-stable",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Shell out to headless Chromium to snapshot `url` at `width x height` and
/// return the resulting PNG bytes. Returns `None` on any failure; callers
/// are expected to treat that as "leave the layer blank".
fn snapshot_url_with_chromium(url: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    let Some(chrome) = find_chromium_binary() else {
        log!("UIWebView bridge: no Chromium binary found; leaving layer blank");
        return None;
    };
    let idx = SNAP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("touchhle_uiwebview_{}.png", idx));
    // Chromium refuses to run as root unless given --no-sandbox.
    let status = Command::new(&chrome)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg(format!("--window-size={},{}", width, height))
        .arg(format!("--screenshot={}", tmp.display()))
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            log!("UIWebView bridge: chromium exited with {}", s);
        }
        Err(e) => {
            log!("UIWebView bridge: failed to spawn chromium: {}", e);
            return None;
        }
    }
    let bytes = std::fs::read(&tmp).ok();
    let _ = std::fs::remove_file(&tmp);
    bytes
}

/// Snapshot `url` and install the decoded PNG as the UIWebView's
/// `layer.contents` so the user sees the rendered web page.
fn render_url_to_layer(env: &mut Environment, this: id, url: &str, frame: CGRect) {
    if url.is_empty()
        || !(url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("file://"))
    {
        return;
    }
    let width = (frame.size.width.max(1.0) as u32).max(1);
    let height = (frame.size.height.max(1.0) as u32).max(1);
    let Some(png) = snapshot_url_with_chromium(url, width, height) else {
        return;
    };
    let Ok(image) = Image::from_bytes(&png) else {
        log!("UIWebView bridge: could not decode PNG snapshot");
        return;
    };
    let cg_image = cg_image::from_image(env, image);
    let layer: id = msg![env; this layer];
    let _: () = msg![env; layer setContents:cg_image];
    let _: () = msg![env; this setNeedsDisplay];
    cg_image::CGImageRelease(env, cg_image);
}

// =========================================================================
// MARK: - Native overlay plumbing
// =========================================================================

/// Hide (and forget) the native overlay backing `this`, if any. Called from
/// `UIView`'s `removeFromSuperview` when the removed view is a UIWebView.
pub(crate) fn webview_did_remove_from_superview(env: &mut Environment, this: id) {
    let overlay_id = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if overlay_id >= 0 {
        android_web_view::hide(overlay_id);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).overlay_id = -1;
    }
}

/// Map the webview's guest frame to host window pixels and create/position
/// the native overlay. Returns the overlay id, or `None` if the view has no
/// on-screen extent (or native overlays are unavailable).
fn overlay_show_or_update(env: &mut Environment, this: id) -> Option<i32> {
    if !android_web_view::native_webview_available() {
        return None;
    }
    let frame: CGRect = msg![env; this frame];
    let (x, y, w, h) = env.window().guest_frame_to_window_px(frame);
    if w <= 0 || h <= 0 {
        log!("UIWebView overlay skipped: view has no on-screen extent yet");
        return None;
    }
    let current = env.objc.borrow::<UIWebViewHostObject>(this).overlay_id;
    if current == -2 {
        // First show: create the overlay seeded with a blank page; the
        // caller immediately navigates or loads data into it.
        let oid = android_web_view::show("about:blank", x, y, w, h);
        env.objc.borrow_mut::<UIWebViewHostObject>(this).overlay_id = oid;
        Some(oid)
    } else if current >= 0 {
        android_web_view::set_bounds(current, x, y, w, h);
        Some(current)
    } else {
        None
    }
}

/// Fire `webViewDidFinishLoad:` on the delegate (if it responds).
fn fire_did_finish_load(env: &mut Environment, this: id) {
    let delegate = env.objc.borrow::<UIWebViewHostObject>(this).delegate;
    if delegate != nil {
        let sel = env.objc.register_host_selector(
            "webViewDidFinishLoad:".to_string(),
            &mut env.mem,
        );
        let responds: bool = msg![env; delegate respondsToSelector:sel];
        if responds {
            let _: () = msg![env; delegate webViewDidFinishLoad:this];
        }
    }
}

/// Fire `webViewDidStartLoad:` on the delegate (if it responds).
fn fire_did_start_load(env: &mut Environment, this: id) {
    let delegate = env.objc.borrow::<UIWebViewHostObject>(this).delegate;
    if delegate != nil {
        let sel = env.objc.register_host_selector(
            "webViewDidStartLoad:".to_string(),
            &mut env.mem,
        );
        let responds: bool = msg![env; delegate respondsToSelector:sel];
        if responds {
            let _: () = msg![env; delegate webViewDidStartLoad:this];
        }
    }
}

/// Schedule the async `webViewDidFinishLoad:` callback. On Android a real
/// page load is in flight, so we delay briefly (matching real-UIWebView
/// timing); the timer holds a retain of `this` which is released in
/// `touchhleWebViewLoadDidFinish:`.
fn schedule_did_finish_load(env: &mut Environment, this: id) {
    retain(env, this);
    let sel = env
        .objc
        .register_host_selector("touchhleWebViewLoadDidFinish:".to_string(), &mut env.mem);
    let _timer: id = msg_class![env;
        NSTimer scheduledTimerWithTimeInterval:0.6
        target:this
        selector:sel
        userInfo:nil
        repeats:false
    ];
}

/// Show (or update) the native overlay and load `url` / `data` into it.
/// `url` is `Some` for URL loads; `data` is `(payload, mime)` for data loads.
/// Returns `true` when the native path was taken.
fn overlay_load(
    env: &mut Environment,
    this: id,
    url: Option<&str>,
    data: Option<(&str, &str)>,
) -> bool {
    let Some(overlay) = overlay_show_or_update(env, this) else {
        return false;
    };
    if let Some((payload, mime)) = data {
        android_web_view::load_data(overlay, payload, mime);
    } else if let Some(url) = url {
        if url.is_empty() {
            android_web_view::load_data(overlay, "<html><body></body></html>", "text/html");
        } else {
            android_web_view::navigate(overlay, url);
        }
    }
    true
}

/// Mark the load as finished and fire `webViewDidFinishLoad:`.
fn finish_load(env: &mut Environment, this: id) {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).loading = false;
    fire_did_finish_load(env, this);
}

/// Android's `evaluateJavascript` returns a JSON-encoded value; unwrap the
/// common cases (quoted string, null, bare literals) into a plain string.
fn unwrap_js_result(result: &str) -> String {
    let t = result.trim();
    if t == "null" {
        return String::new();
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        let inner = &t[1..t.len() - 1];
        return inner
            .replace("\\\\", "\u{1}")
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\t", "\t")
            .replace("\\\"","\"")
            .replace('\u{1}', "\\");
    }
    t.to_string()
}
