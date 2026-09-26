/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CAEAGLLayer`.

use super::ca_layer::CALayerHostObject;
use crate::frameworks::core_graphics::cg_affine_transform::{
    CGAffineTransform, CGAffineTransformIdentity,
};
use crate::frameworks::core_graphics::{CGPoint, CGRect};
use crate::frameworks::foundation::ns_string;
use crate::objc::{id, msg, msg_class, nil, objc_classes, release, Class, ClassExports};
use crate::Environment;

// MARK: - EAGLDrawable property key constants
//
// These are the keys apps put in the drawableProperties dictionary.
// We export them as static strings so other modules can reference them.
pub const kEAGLDrawablePropertyRetainedBacking: &str = "RetainedBacking";
pub const kEAGLDrawablePropertyColorFormat: &str = "ColorFormat";

// kEAGLColorFormat values
pub const kEAGLColorFormatRGBA8: &str = "RGBA8";
pub const kEAGLColorFormatRGB565: &str = "RGB565";
pub const kEAGLColorFormatSRGBA8: &str = "SRGBA8";

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CAEAGLLayer: CALayer

// =========================================================================
// MARK: - EAGLDrawable protocol
// =========================================================================

- (id)drawableProperties { // NSDictionary<NSString*, id>*
    env.objc.borrow::<CALayerHostObject>(this).drawable_properties
}

- (())setDrawableProperties:(id)props { // NSDictionary<NSString*, id>*
    // Store a copy — matches Apple's behaviour.
    let old = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    let new_props: id = if props != nil { msg![env; props copy] } else { nil };
    release(env, old);
    env.objc.borrow_mut::<CALayerHostObject>(this).drawable_properties = new_props;

    // Log the properties the app is requesting so rendering issues are
    // easier to diagnose.
    if new_props != nil {
        let retained_key = ns_string::get_static_str(env, kEAGLDrawablePropertyRetainedBacking);
        let format_key    = ns_string::get_static_str(env, kEAGLDrawablePropertyColorFormat);

        let retained_val: id = msg![env; new_props objectForKey:retained_key];
        let format_val:   id = msg![env; new_props objectForKey:format_key];

        let retained_str = if retained_val != nil {
            let b: bool = msg![env; retained_val boolValue];
            if b { "YES" } else { "NO" }
        } else {
            "(not set)"
        };

        let format_str = if format_val != nil {
            ns_string::to_rust_string(env, format_val).into_owned()
        } else {
            "(not set)".to_string()
        };

        log_dbg!(
            "CAEAGLLayer setDrawableProperties: retainedBacking={} colorFormat={}",
            retained_str, format_str
        );
    }
}

// =========================================================================
// MARK: - Convenience helpers (read individual drawable properties)
// =========================================================================

// Returns YES if the backing store should be retained after presentation.
// Corresponds to kEAGLDrawablePropertyRetainedBacking.
- (bool)_touchHLE_retainedBacking {
    let props = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    if props == nil { return false; }
    let key: id = ns_string::get_static_str(env, kEAGLDrawablePropertyRetainedBacking);
    let val: id = msg![env; props objectForKey:key];
    if val == nil { return false; }
    msg![env; val boolValue]
}

// Returns the requested color format string, or "kEAGLColorFormatRGBA8"
// as the default if not specified.
- (id)_touchHLE_colorFormat { // NSString*
    let props = env.objc.borrow::<CALayerHostObject>(this).drawable_properties;
    if props != nil {
        let key: id = ns_string::get_static_str(env, kEAGLDrawablePropertyColorFormat);
        let val: id = msg![env; props objectForKey:key];
        if val != nil { return val; }
    }
    ns_string::get_static_str(env, kEAGLColorFormatRGBA8)
}

// =========================================================================
// MARK: - CALayer overrides
// =========================================================================

// CAEAGLLayer is always opaque by default (unlike plain CALayer).
- (id)init {
    let _: () = msg![env; this setOpaque:true];
    this
}

// MARK: - Scale / Retina Support

// NOTE: unlike the previous hardcoded-1.0 stubs, contentsScale is NOT
// overridden here. It is inherited from CALayer, whose host object stores a
// real `contents_scale` field. `UIView.init_common` seeds every view's
// backing layer with the main screen's scale, so on retina devices (iPhone
// 4/4s/5/5c, iPod touch 4/5, iPad 3/4/5/mini 2/3) the EAGL renderbuffer is
// allocated at bounds * 2.0 and games render at native resolution instead of
// being zoomed/cropped into a half-size framebuffer. Apps that explicitly
// call setContentScaleFactor: / setContentsScale: round-trip correctly.

- (id)initWithLayer:(id)layer {
    let _: () = msg![env; this setOpaque:true];
    // Copy drawable properties from the source layer if it is also a
    // CAEAGLLayer.
    let ca_eagl_class: Class = msg_class![env; CAEAGLLayer class];
    let is_eagl: bool = msg![env; layer isKindOfClass:ca_eagl_class];
    if is_eagl {
        let src_props = env.objc.borrow::<CALayerHostObject>(layer).drawable_properties;
        if src_props != nil {
            let copy: id = msg![env; src_props copy];
            env.objc.borrow_mut::<CALayerHostObject>(this).drawable_properties = copy;
        }
    }
    this
}

// Prevent the layer from being drawn by Core Animation — its contents are
// managed exclusively by EAGL/OpenGL ES.
- (())display {
    // No-op: the layer is presented via EAGLContext presentRenderBuffer:.
}

- (())drawInContext:(id)_ctx { // CGContextRef
    // No-op: CAEAGLLayer content comes from OpenGL ES, not Core Graphics.
}

// =========================================================================
// MARK: - Description
// =========================================================================

- (id)description {
    let host = env.objc.borrow::<CALayerHostObject>(this);
    let opaque = host.opaque;
    let has_props = host.drawable_properties != nil;
    let s = format!(
        "<CAEAGLLayer: {:?}; opaque={}; drawableProperties={}>",
        this,
        opaque,
        if has_props { "(set)" } else { "(nil)" }
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};

// =========================================================================
// MARK: - find_fullscreen_eagl_layer
// =========================================================================

/// Layer transforms that can still be presented directly to the host window.
/// The only non-identity transform UIKit applies to a fullscreen app view in
/// our compatibility layer is the device-orientation rotation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FullscreenLayerTransform {
    Identity,
    DeviceRotation,
}

fn nearly_equal(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

fn nearly_equal_transform(a: CGAffineTransform, b: CGAffineTransform) -> bool {
    let CGAffineTransform {
        a: aa,
        b: ab,
        c: ac,
        d: ad,
        tx: atx,
        ty: aty,
    } = a;
    let CGAffineTransform {
        a: ba,
        b: bb,
        c: bc,
        d: bd,
        tx: btx,
        ty: bty,
    } = b;
    const TOLERANCE: f32 = 1.0e-5;
    nearly_equal(aa, ba, TOLERANCE)
        && nearly_equal(ab, bb, TOLERANCE)
        && nearly_equal(ac, bc, TOLERANCE)
        && nearly_equal(ad, bd, TOLERANCE)
        && nearly_equal(atx, btx, TOLERANCE)
        && nearly_equal(aty, bty, TOLERANCE)
}

fn classify_fullscreen_layer_transform(
    transform: CGAffineTransform,
    orientation: crate::window::DeviceOrientation,
) -> Option<FullscreenLayerTransform> {
    if nearly_equal_transform(transform, CGAffineTransformIdentity) {
        return Some(FullscreenLayerTransform::Identity);
    }

    let angle = match orientation {
        crate::window::DeviceOrientation::Portrait => return None,
        crate::window::DeviceOrientation::PortraitUpsideDown => std::f32::consts::PI,
        crate::window::DeviceOrientation::LandscapeLeft => -std::f32::consts::FRAC_PI_2,
        crate::window::DeviceOrientation::LandscapeRight => std::f32::consts::FRAC_PI_2,
    };
    nearly_equal_transform(transform, CGAffineTransform::make_rotation(angle))
        .then_some(FullscreenLayerTransform::DeviceRotation)
}

fn fullscreen_frame_matches_screen(frame: CGRect, screen: CGRect) -> bool {
    let CGRect {
        origin: frame_origin,
        size: frame_size,
    } = frame;
    let CGRect {
        origin: screen_origin,
        size: screen_size,
    } = screen;
    // Rotating an integer-sized UIKit view can leave a few floating-point
    // ulps in `frame`; a hundredth of a point is far below a visible pixel.
    const TOLERANCE: f32 = 0.01;
    nearly_equal(frame_origin.x, screen_origin.x, TOLERANCE)
        && nearly_equal(frame_origin.y, screen_origin.y, TOLERANCE)
        && nearly_equal(frame_size.width, screen_size.width, TOLERANCE)
        && nearly_equal(frame_size.height, screen_size.height, TOLERANCE)
}

/// If there is an opaque `CAEAGLLayer` that covers the entire screen, this
/// returns a pointer to it. Otherwise, it returns [nil].
pub fn find_fullscreen_eagl_layer(env: &mut Environment) -> id {
    if env.options.force_composition {
        return nil;
    }

    let windows = env.framework_state.uikit.ui_view.ui_window.windows.clone();
    let Some(top_window) = windows
        .into_iter()
        .rev()
        .find(|&window| !msg![env; window isHidden])
    else {
        return nil;
    };

    let screen_bounds: CGRect = {
        let screen: id = msg_class![env; UIScreen mainScreen];
        msg![env; screen bounds]
    };

    let Some(orientation) = env.window.as_ref().map(|window| window.current_rotation()) else {
        return nil;
    };
    let mut layer: id = msg![env; top_window layer];
    // Accumulate each layer's local-to-superlayer transform so child EAGL
    // layers inside an autorotated root view are checked in screen space too.
    let mut parent_to_screen = CGAffineTransformIdentity;
    let mut saw_device_rotation = false;

    loop {
        // assert!(layer != nil);

        let layer_host_obj: &CALayerHostObject = env.objc.borrow(layer);
        let transform_kind = classify_fullscreen_layer_transform(
            layer_host_obj.affine_transform,
            orientation,
        );
        let layer_bounds = layer_host_obj.bounds;
        let layer_to_screen = layer_host_obj
            .superlayer_to_layer_transform()
            .concat(parent_to_screen);
        let layer_frame = layer_to_screen.apply_to_rect(CGRect {
            origin: layer_bounds.origin,
            size: layer_bounds.size,
        });

        // UIKit's iOS 5-style autorotation sets the root view's transform to
        // the device rotation, then resets its frame to the full-screen bounds.
        // Requiring an identity transform here made landscape EAGL views look
        // non-fullscreen, so `presentRenderbuffer:` fell back to glReadPixels
        // plus software Core Animation composition on every frame. Accept one
        // exact device-rotation transform, but only when the transformed frame
        // still covers the whole screen. The GPU presenter applies the
        // configured orientation compensation itself. Arbitrary transforms,
        // scaled views and nested/double rotations keep the safe compositor
        // path.
        let transform_is_usable = match transform_kind {
            Some(FullscreenLayerTransform::Identity) => true,
            Some(FullscreenLayerTransform::DeviceRotation) if !saw_device_rotation => {
                saw_device_rotation = true;
                true
            }
            _ => false,
        };
        if !fullscreen_frame_matches_screen(layer_frame, screen_bounds)
            || layer_bounds.origin != (CGPoint { x: 0.0, y: 0.0 })
            || layer_host_obj.anchor_point != (CGPoint { x: 0.5, y: 0.5 })
            || layer_host_obj.hidden
            || layer_host_obj.opacity != 1.0
            || !transform_is_usable
        {
            return nil;
        }

        parent_to_screen = layer_to_screen;
        if let Some(&next) = layer_host_obj.sublayers.last() {
            layer = next;
        } else {
            break;
        }
    }

    if !env.objc.borrow::<CALayerHostObject>(layer).opaque {
        return nil;
    }

    let ca_eagl_layer_class: Class = msg_class![env; CAEAGLLayer class];
    if !msg![env; layer isKindOfClass:ca_eagl_layer_class] {
        return nil;
    }

    layer
}

#[cfg(test)]
mod fullscreen_layer_tests {
    use super::*;
    use crate::window::DeviceOrientation;

    #[test]
    fn accepts_identity_and_the_current_device_rotation_only() {
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransformIdentity,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::Identity),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(-std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_2),
                DeviceOrientation::LandscapeRight,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
    }

    #[test]
    fn rejects_transforms_that_change_fullscreen_coverage() {
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_rotation(std::f32::consts::FRAC_PI_4),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_scale(0.9, 1.0),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                CGAffineTransform::make_translation(1.0, 0.0),
                DeviceOrientation::LandscapeLeft,
            ),
            None,
        );
    }

    #[test]
    fn autorotated_child_eagl_layer_covers_a_portrait_uikit_screen() {
        let screen = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.0,
                height: 568.0,
            },
        };
        let mut window_layer = CALayerHostObject::default();
        window_layer.bounds = screen;
        window_layer.position = CGPoint { x: 160.0, y: 284.0 };
        window_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        window_layer.affine_transform = CGAffineTransformIdentity;
        let window_to_screen = window_layer.superlayer_to_layer_transform();

        let mut root_view_layer = CALayerHostObject::default();
        root_view_layer.bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 568.0,
                height: 320.0,
            },
        };
        root_view_layer.position = CGPoint { x: 160.0, y: 284.0 };
        root_view_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        root_view_layer.affine_transform =
            CGAffineTransform::make_rotation(-std::f32::consts::FRAC_PI_2);
        let root_view_to_screen = root_view_layer
            .superlayer_to_layer_transform()
            .concat(window_to_screen);

        let mut eagl_layer = CALayerHostObject::default();
        eagl_layer.bounds = root_view_layer.bounds;
        eagl_layer.position = CGPoint { x: 284.0, y: 160.0 };
        eagl_layer.anchor_point = CGPoint { x: 0.5, y: 0.5 };
        eagl_layer.affine_transform = CGAffineTransformIdentity;
        let eagl_to_parent = eagl_layer.superlayer_to_layer_transform();
        let eagl_to_screen = eagl_to_parent.concat(root_view_to_screen);
        let eagl_bounds = eagl_layer.bounds;
        let eagl_bounds_rect = CGRect {
            origin: eagl_bounds.origin,
            size: eagl_bounds.size,
        };
        let local_frame = eagl_to_parent.apply_to_rect(eagl_bounds_rect);
        let screen_frame = eagl_to_screen.apply_to_rect(eagl_bounds_rect);

        assert_eq!(
            classify_fullscreen_layer_transform(
                root_view_layer.affine_transform,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::DeviceRotation),
        );
        assert_eq!(
            classify_fullscreen_layer_transform(
                eagl_layer.affine_transform,
                DeviceOrientation::LandscapeLeft,
            ),
            Some(FullscreenLayerTransform::Identity),
        );
        assert!(!fullscreen_frame_matches_screen(local_frame, screen));
        assert!(fullscreen_frame_matches_screen(screen_frame, screen));
    }

    #[test]
    fn rejects_frames_that_do_not_cover_the_screen() {
        let screen = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.0,
                height: 568.0,
            },
        };
        let almost_fullscreen = CGRect {
            origin: CGPoint { x: -0.002, y: 0.003 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 320.001,
                height: 567.999,
            },
        };
        let partial = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: crate::frameworks::core_graphics::CGSize {
                width: 319.0,
                height: 568.0,
            },
        };

        assert!(fullscreen_frame_matches_screen(almost_fullscreen, screen));
        assert!(!fullscreen_frame_matches_screen(partial, screen));
    }
}

// =========================================================================
// MARK: - Pixel buffer helpers (used by EAGLContext)
// =========================================================================

/// Takes the pixel buffer out of the layer so it can be refilled by
/// `EAGLContext presentRenderBuffer:`. Pass the buffer back via
/// [present_pixels] once it is filled.
pub fn get_pixels_vec_for_presenting(env: &mut Environment, layer: id) -> Vec<u8> {
    env.objc
        .borrow_mut::<CALayerHostObject>(layer)
        .presented_pixels
        .take()
        .map(|(vec, _w, _h)| vec)
        .unwrap_or_default()
}

/// Stores the new rendered frame in the layer and marks the GLES texture as
/// stale. Data must be in RGBA8 format.
pub fn present_pixels(env: &mut Environment, layer: id, pixels: Vec<u8>, width: u32, height: u32) {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(layer);
    host_obj.presented_pixels = Some((pixels, width, height));
    host_obj.gles_texture_is_up_to_date = false;
}

/// Returns whether the layer's backing store should be retained after
/// presentation (i.e. `kEAGLDrawablePropertyRetainedBacking` is YES).
/// Convenience wrapper for use by `EAGLContext`.
pub fn is_retained_backing(env: &mut Environment, layer: id) -> bool {
    msg![env; layer _touchHLE_retainedBacking]
}

/// Returns the drawable's pixel width (from the presented pixel buffer if
/// available, otherwise from the layer's bounds).
pub fn drawable_width(env: &mut Environment, layer: id) -> u32 {
    let host = env.objc.borrow::<CALayerHostObject>(layer);
    if let Some((_, w, _)) = host.presented_pixels {
        return w;
    }
    host.bounds.size.width as u32
}

/// Returns the drawable's pixel height.
pub fn drawable_height(env: &mut Environment, layer: id) -> u32 {
    let host = env.objc.borrow::<CALayerHostObject>(layer);
    if let Some((_, _, h)) = host.presented_pixels {
        return h;
    }
    host.bounds.size.height as u32
}
