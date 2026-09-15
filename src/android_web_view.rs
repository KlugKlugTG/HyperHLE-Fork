/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Bridge from the guest's `UIWebView` to a *real* native Android WebView.
//!
//! On Android, each `UIWebView` is backed by a genuine `android.webkit.WebView`
//! layered on top of the SDL surface, so pages load with a real engine
//! (HTML/CSS/JS, HTTPS, links, scrolling, back/forward). All calls go through
//! hand-rolled JNI (no `jni` crate dependency) into static methods on
//! `MainActivity` — see `android/app/src/main/java/org/touchhle/android/`.
//!
//! The MainActivity class reference and the method IDs of every bridge method
//! are resolved once at startup by `populate_jni_cache`, on the real
//! SDLThread stack (guest code later calls this bridge from a coroutine
//! stack, where JNI lookups are not reliable).
//!
//! On other platforms every function here is a harmless no-op; the desktop
//! build renders webviews with the headless-Chromium snapshot bridge in
//! `ui_web_view.rs` instead.


/// A live overlay id, or `-1` when no overlay could be created.
pub type OverlayId = i32;

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};
    use std::sync::OnceLock;

    extern "C" {
        // Exported by libSDL2.so on Android (SDL_system.h). Returns the
        // JNIEnv* for the calling thread, attaching it to the JVM if needed.
        fn SDL_AndroidGetJNIEnv() -> *mut c_void;
        // Exported by libSDL2.so on Android (SDL_system.h). Returns the
        // current Activity instance as a jobject (a local reference). SDL
        // resolves it through a class reference cached at startup, so this
        // needs no FindClass/class-loader lookup on our side.
        fn SDL_AndroidGetActivity() -> *mut c_void;
    }

    /// JNI function-table slot indices: the position of each function
    /// pointer inside `struct JNINativeInterface_` from jni.h. The layout,
    /// including the four reserved pointers at slots 0-3, is a stable ABI
    /// shared by ART and desktop JVMs, so an index is simply the position
    /// of the function in the struct. A slot that is off by a few entries
    /// silently resolves to a *different* JNI function, which ends in a
    /// native crash (SIGSEGV), so verify against jni.h when editing.
    mod slots {
        pub const FIND_CLASS: usize = 6;
        pub const EXCEPTION_OCCURRED: usize = 15;
        pub const EXCEPTION_CLEAR: usize = 17;
        pub const NEW_GLOBAL_REF: usize = 21;
        pub const DELETE_LOCAL_REF: usize = 23;
        pub const GET_OBJECT_CLASS: usize = 31;
        pub const GET_STATIC_METHOD_ID: usize = 113;
        pub const CALL_STATIC_OBJECT_METHOD_A: usize = 116;
        pub const CALL_STATIC_BOOLEAN_METHOD_A: usize = 119;
        pub const CALL_STATIC_INT_METHOD_A: usize = 131;
        pub const CALL_STATIC_VOID_METHOD_A: usize = 143;
        pub const NEW_STRING_UTF: usize = 167;
        pub const GET_STRING_UTF_CHARS: usize = 169;
        pub const RELEASE_STRING_UTF_CHARS: usize = 170;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    union JValue {
        l: *mut c_void,
        i: c_int,
        z: u8,
        _pad: u64,
    }

    /// One JNI call context: the JNIEnv and a couple of vtable fns.
    struct Jni {
        env: *mut c_void,
    }

    impl Jni {
        fn attach() -> Option<Jni> {
            unsafe {
                let env = SDL_AndroidGetJNIEnv();
                if env.is_null() {
                    return None;
                }
                Some(Jni { env })
            }
        }

        /// `self.env` is a `JNIEnv*`: it points at the JNIEnv struct, whose
        /// only field is the pointer to the `JNINativeInterface_` function
        /// table. One dereference is needed to reach the table (this is the
        /// `(*env)->Fn(env, ...)` idiom from C). Indexing `env` itself would
        /// read JNIEnv-internal fields and try to call them as function
        /// pointers, crashing the host process with SIGSEGV.
        fn slot<F>(&self, index: usize) -> F {
            unsafe {
                let table = *(self.env as *mut *mut c_void) as *mut *mut c_void;
                std::mem::transmute_copy::<*mut c_void, F>(&*table.add(index))
            }
        }

        fn exception_pending(&self) -> bool {
            let f: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                self.slot(slots::EXCEPTION_OCCURRED);
            let thrown = unsafe { f(self.env) };
            !thrown.is_null()
        }

        fn clear_exception(&self) {
            let f: unsafe extern "C" fn(*mut c_void) = self.slot(slots::EXCEPTION_CLEAR);
            unsafe { f(self.env) }
        }

        fn find_main_activity_class(&self) -> Option<*mut c_void> {
            let name = CString::new("org/touchhle/android/MainActivity").ok()?;
            let f: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
                self.slot(slots::FIND_CLASS);
            let class = unsafe { f(self.env, name.as_ptr()) };
            if class.is_null() {
                self.clear_exception();
                return None;
            }
            Some(class)
        }

        fn get_static_method(
            &self,
            class: *mut c_void,
            name: &str,
            sig: &str,
        ) -> Option<*mut c_void> {
            let name = CString::new(name).ok()?;
            let sig = CString::new(sig).ok()?;
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *const c_char,
                *const c_char,
            ) -> *mut c_void = self.slot(slots::GET_STATIC_METHOD_ID);
            let method = unsafe { f(self.env, class, name.as_ptr(), sig.as_ptr()) };
            if method.is_null() {
                self.clear_exception();
                return None;
            }
            Some(method)
        }

        fn new_global_ref(&self, obj: *mut c_void) -> *mut c_void {
            if obj.is_null() {
                return std::ptr::null_mut();
            }
            let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                self.slot(slots::NEW_GLOBAL_REF);
            unsafe { f(self.env, obj) }
        }

        fn get_object_class(&self, obj: *mut c_void) -> *mut c_void {
            if obj.is_null() {
                return std::ptr::null_mut();
            }
            let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                self.slot(slots::GET_OBJECT_CLASS);
            unsafe { f(self.env, obj) }
        }

        /// Get the MainActivity class as a global reference. The preferred
        /// route takes the runtime class of the live Activity instance
        /// (SDL_AndroidGetActivity + GetObjectClass), which involves no
        /// FindClass and therefore no class-loader resolution. FindClass is
        /// only a fallback: it resolves the class against the calling Java
        /// frame's class loader, which is not reliable from the native
        /// frames touchHLE runs in.
        fn main_activity_global_class(&self) -> Option<*mut c_void> {
            let mut local = std::ptr::null_mut();
            let activity = unsafe { SDL_AndroidGetActivity() };
            if !activity.is_null() {
                local = self.get_object_class(activity);
                self.delete_local_ref(activity);
            }
            if local.is_null() {
                local = self.find_main_activity_class()?;
            }
            let global = self.new_global_ref(local);
            self.delete_local_ref(local);
            if global.is_null() {
                None
            } else {
                Some(global)
            }
        }

        fn new_jstring(&self, s: &str) -> *mut c_void {
            // JNI's NewStringUTF expects "modified UTF-8", not plain UTF-8;
            // see to_modified_utf8 below. Its output never contains a NUL
            // byte, so CString::new cannot fail.
            let Ok(c) = CString::new(to_modified_utf8(s)) else {
                return std::ptr::null_mut();
            };
            let f: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
                self.slot(slots::NEW_STRING_UTF);
            unsafe { f(self.env, c.as_ptr()) }
        }

        fn jstring_to_rust(&self, js: *mut c_void) -> Option<String> {
            if js.is_null() {
                return None;
            }
            let get: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> *const c_char =
                self.slot(slots::GET_STRING_UTF_CHARS);
            let release: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) =
                self.slot(slots::RELEASE_STRING_UTF_CHARS);
            let chars = unsafe { get(self.env, js, std::ptr::null_mut()) };
            if chars.is_null() {
                return None;
            }
            let s = unsafe { std::ffi::CStr::from_ptr(chars) }
                .to_string_lossy()
                .into_owned();
            unsafe { release(self.env, js, chars) };
            Some(s)
        }

        fn delete_local_ref(&self, obj: *mut c_void) {
            if obj.is_null() {
                return;
            }
            let f: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                self.slot(slots::DELETE_LOCAL_REF);
            unsafe { f(self.env, obj) }
        }

        fn call_static_void(&self, class: *mut c_void, method: *mut c_void, args: &[JValue]) {
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *const JValue,
            ) = self.slot(slots::CALL_STATIC_VOID_METHOD_A);
            unsafe { f(self.env, class, method, args.as_ptr()) };
            if self.exception_pending() {
                self.clear_exception();
            }
        }

        fn call_static_bool(
            &self,
            class: *mut c_void,
            method: *mut c_void,
            args: &[JValue],
        ) -> bool {
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *const JValue,
            ) -> u8 = self.slot(slots::CALL_STATIC_BOOLEAN_METHOD_A);
            let r = unsafe { f(self.env, class, method, args.as_ptr()) };
            if self.exception_pending() {
                self.clear_exception();
            }
            r != 0
        }

        fn call_static_int(
            &self,
            class: *mut c_void,
            method: *mut c_void,
            args: &[JValue],
        ) -> c_int {
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *const JValue,
            ) -> c_int = self.slot(slots::CALL_STATIC_INT_METHOD_A);
            let r = unsafe { f(self.env, class, method, args.as_ptr()) };
            if self.exception_pending() {
                self.clear_exception();
            }
            r
        }

        fn call_static_object(
            &self,
            class: *mut c_void,
            method: *mut c_void,
            args: &[JValue],
        ) -> *mut c_void {
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *const JValue,
            ) -> *mut c_void = self.slot(slots::CALL_STATIC_OBJECT_METHOD_A);
            let r = unsafe { f(self.env, class, method, args.as_ptr()) };
            if self.exception_pending() {
                self.clear_exception();
            }
            r
        }
    }

    fn int_args(id: OverlayId) -> [JValue; 1] {
        [JValue { i: id }]
    }

    /// Convert Rust UTF-8 into the "modified UTF-8" that JNI expects:
    /// U+0000 is encoded as C0 80 and supplementary characters (above
    /// U+FFFF) as CESU-8 surrogate pairs. Plain multi-byte UTF-8 is only
    /// valid input for NewStringUTF when it is pure ASCII, and ART's
    /// CheckJNI aborts the process on invalid input.
    fn to_modified_utf8(s: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(s.len());
        for c in s.chars() {
            let cp = c as u32;
            if cp == 0 {
                out.extend_from_slice(&[0xC0, 0x80]);
            } else if cp <= 0x7F {
                out.push(cp as u8);
            } else if cp <= 0x7FF {
                out.push((0xC0 | (cp >> 6)) as u8);
                out.push((0x80 | (cp & 0x3F)) as u8);
            } else if cp <= 0xFFFF {
                out.push((0xE0 | (cp >> 12)) as u8);
                out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
                out.push((0x80 | (cp & 0x3F)) as u8);
            } else {
                // Encode as a surrogate pair (CESU-8).
                let v = cp - 0x1_0000;
                for x in [0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)] {
                    out.push((0xE0 | (x >> 12)) as u8);
                    out.push((0x80 | ((x >> 6) & 0x3F)) as u8);
                    out.push((0x80 | (x & 0x3F)) as u8);
                }
            }
        }
        out
    }

    /// The MainActivity class (as a global reference) plus the method IDs
    /// of every static bridge method, resolved once at startup.
    struct JniCache {
        class: *mut c_void,
        show: *mut c_void,
        navigate: *mut c_void,
        load_data: *mut c_void,
        set_bounds: *mut c_void,
        hide: *mut c_void,
        go_back: *mut c_void,
        go_forward: *mut c_void,
        can_go_back: *mut c_void,
        can_go_forward: *mut c_void,
        eval_js: *mut c_void,
        stop_loading: *mut c_void,
        open_url: *mut c_void,
    }

    // SAFETY: the class is held as a global reference, which is valid from
    // any thread until the process exits, and jmethodIDs are process-wide
    // lifetime handles, so sharing this cache across threads is sound.
    unsafe impl Send for JniCache {}
    unsafe impl Sync for JniCache {}

    static CACHE: OnceLock<JniCache> = OnceLock::new();

    fn cached() -> Option<&'static JniCache> {
        CACHE.get()
    }

    /// Resolve the MainActivity class and all bridge method IDs once, on
    /// the real SDLThread stack, before guest emulation begins. This must
    /// not be called from guest code, which runs on a coroutine stack where
    /// JNI is unreliable. If any step fails, the failure is logged, the
    /// cache stays empty and the WebView falls back to the desktop
    /// Chromium snapshot path.
    pub fn populate_jni_cache() {
        fn method(jni: &Jni, class: *mut c_void, name: &str, sig: &str) -> Option<*mut c_void> {
            let mid = jni.get_static_method(class, name, sig);
            if mid.is_none() {
                log!("GetStaticMethodID({}) failed; native WebView disabled", name);
            }
            mid
        }
        fn resolve(jni: &Jni) -> Option<JniCache> {
            let class = match jni.main_activity_global_class() {
                Some(class) => class,
                None => {
                    log!("MainActivity class not found; native WebView disabled");
                    return None;
                }
            };
            Some(JniCache {
                class,
                show: method(jni, class, "showWebOverlay", "(Ljava/lang/String;IIII)I")?,
                navigate: method(jni, class, "navigateWebOverlay", "(ILjava/lang/String;)V")?,
                load_data: method(
                    jni,
                    class,
                    "loadDataWebOverlay",
                    "(ILjava/lang/String;Ljava/lang/String;)V",
                )?,
                set_bounds: method(jni, class, "setWebOverlayBounds", "(IIIII)V")?,
                hide: method(jni, class, "hideWebOverlay", "(I)V")?,
                go_back: method(jni, class, "goBackWebOverlay", "(I)V")?,
                go_forward: method(jni, class, "goForwardWebOverlay", "(I)V")?,
                can_go_back: method(jni, class, "canGoBackWebOverlay", "(I)Z")?,
                can_go_forward: method(jni, class, "canGoForwardWebOverlay", "(I)Z")?,
                eval_js: method(
                    jni,
                    class,
                    "evalJsWebOverlay",
                    "(ILjava/lang/String;)Ljava/lang/String;",
                )?,
                stop_loading: method(jni, class, "stopLoadingWebOverlay", "(I)V")?,
                open_url: method(jni, class, "openExternalUrl", "(Ljava/lang/String;)I")?,
            })
        }
        let Some(jni) = Jni::attach() else {
            log!("No JNIEnv from SDL; native WebView disabled");
            return;
        };
        if let Some(cache) = resolve(&jni) {
            log!("Native WebView bridge ready");
            let _ = CACHE.set(cache);
        }
    }

    pub fn show(url: &str, x: i32, y: i32, w: i32, h: i32) -> OverlayId {
        let Some(jni) = Jni::attach() else {
            return -1;
        };
        let Some(c) = cached() else {
            return -1;
        };
        let url_j = jni.new_jstring(url);
        let args = [
            JValue { l: url_j },
            JValue { i: x },
            JValue { i: y },
            JValue { i: w },
            JValue { i: h },
        ];
        let id = jni.call_static_int(c.class, c.show, &args);
        jni.delete_local_ref(url_j);
        id
    }

    pub fn navigate(id: OverlayId, url: &str) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        let url_j = jni.new_jstring(url);
        let args = [JValue { i: id }, JValue { l: url_j }];
        jni.call_static_void(c.class, c.navigate, &args);
        jni.delete_local_ref(url_j);
    }

    pub fn load_data(id: OverlayId, data: &str, mime: &str) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        let data_j = jni.new_jstring(data);
        let mime_j = jni.new_jstring(mime);
        let args = [JValue { i: id }, JValue { l: data_j }, JValue { l: mime_j }];
        jni.call_static_void(c.class, c.load_data, &args);
        jni.delete_local_ref(data_j);
        jni.delete_local_ref(mime_j);
    }

    pub fn set_bounds(id: OverlayId, x: i32, y: i32, w: i32, h: i32) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        let args = [
            JValue { i: id },
            JValue { i: x },
            JValue { i: y },
            JValue { i: w },
            JValue { i: h },
        ];
        jni.call_static_void(c.class, c.set_bounds, &args);
    }

    pub fn hide(id: OverlayId) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        jni.call_static_void(c.class, c.hide, &int_args(id));
    }

    pub fn go_back(id: OverlayId) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        jni.call_static_void(c.class, c.go_back, &int_args(id));
    }

    pub fn go_forward(id: OverlayId) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        jni.call_static_void(c.class, c.go_forward, &int_args(id));
    }

    pub fn can_go_back(id: OverlayId) -> bool {
        let Some(jni) = Jni::attach() else {
            return false;
        };
        let Some(c) = cached() else {
            return false;
        };
        jni.call_static_bool(c.class, c.can_go_back, &int_args(id))
    }

    pub fn can_go_forward(id: OverlayId) -> bool {
        let Some(jni) = Jni::attach() else {
            return false;
        };
        let Some(c) = cached() else {
            return false;
        };
        jni.call_static_bool(c.class, c.can_go_forward, &int_args(id))
    }

    pub fn eval_js(id: OverlayId, script: &str) -> Option<String> {
        let jni = Jni::attach()?;
        let c = cached()?;
        let script_j = jni.new_jstring(script);
        let args = [JValue { i: id }, JValue { l: script_j }];
        let out = jni.call_static_object(c.class, c.eval_js, &args);
        let s = jni.jstring_to_rust(out);
        jni.delete_local_ref(script_j);
        jni.delete_local_ref(out);
        s
    }

    pub fn stop_loading(id: OverlayId) {
        let Some(jni) = Jni::attach() else {
            return;
        };
        let Some(c) = cached() else {
            return;
        };
        jni.call_static_void(c.class, c.stop_loading, &int_args(id));
    }

    /// Whether the native WebView bridge is usable (used by `ui_web_view.rs`
    /// to decide between the native path and the desktop Chromium fallback).
    pub fn native_webview_available() -> bool {
        CACHE.get().is_some()
    }

    /// Open a URL in the system browser (or whichever app owns the scheme).
    /// Returns true if the intent was launched successfully.
    pub fn open_url_external(url: &str) -> bool {
        let Some(jni) = Jni::attach() else {
            return false;
        };
        let Some(c) = cached() else {
            return false;
        };
        let url_j = jni.new_jstring(url);
        let args = [JValue { l: url_j }];
        let ret = jni.call_static_int(c.class, c.open_url, &args);
        jni.delete_local_ref(url_j);
        ret == 0
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::OverlayId;

    pub fn populate_jni_cache() {}
    pub fn show(_url: &str, _x: i32, _y: i32, _w: i32, _h: i32) -> OverlayId {
        -1
    }
    pub fn navigate(_id: OverlayId, _url: &str) {}
    pub fn load_data(_id: OverlayId, _data: &str, _mime: &str) {}
    pub fn set_bounds(_id: OverlayId, _x: i32, _y: i32, _w: i32, _h: i32) {}
    pub fn hide(_id: OverlayId) {}
    pub fn go_back(_id: OverlayId) {}
    pub fn go_forward(_id: OverlayId) {}
    pub fn can_go_back(_id: OverlayId) -> bool {
        false
    }
    pub fn can_go_forward(_id: OverlayId) -> bool {
        false
    }
    pub fn eval_js(_id: OverlayId, _script: &str) -> Option<String> {
        None
    }
    pub fn stop_loading(_id: OverlayId) {}
    pub fn native_webview_available() -> bool {
        false
    }
    pub fn open_url_external(_url: &str) -> bool {
        false
    }
}

pub use imp::*;
