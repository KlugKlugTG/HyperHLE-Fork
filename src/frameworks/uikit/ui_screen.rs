/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIScreen`.

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::objc::{id, msg, msg_class, nil, objc_classes, ClassExports, TrivialHostObject, SEL};

#[derive(Default)]
pub struct State {
    main_screen: Option<id>,
    current_mode: Option<id>,
    /// `-brightness`, clamped to the documented 0.0-1.0 range.
    brightness: CGFloat,
    /// `-wantsSoftwareDimming`.
    wants_software_dimming: bool,
}

fn screen_size_for_current_orientation(env: &mut crate::Environment) -> (u32, u32) {
    let device_family = env.window().device_family();
    let mut portrait_size = device_family.portrait_size();

    // iOS 4-inch compatibility: apps without Default-568h@2x.png run in
    // 3.5-inch letterbox mode even on iPhone 5/5c (320x480 points).
    if device_family.is_phone_568() {
        let supports_5 = env.bundle.supports_iphone5(&env.fs);
        if !supports_5 {
            portrait_size = (320, 480);
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log!(
                    "UIScreen: app does not support 4-inch (no Default-568h@2x.png), reporting 320x480 bounds even on {:?}",
                    device_family
                );
            });
        }
    }

    let (portrait_width, portrait_height) = portrait_size;

    if crate::env_flag_cached!("TOUCHHLE_LANDSCAPE_UISCREEN_BOUNDS") {
        let is_landscape = !matches!(
            env.window().current_rotation(),
            crate::window::DeviceOrientation::Portrait
        );

        if is_landscape {
            return (portrait_height, portrait_width);
        }
    }

    (portrait_width, portrait_height)
}

/// Per Apple documentation for `-setBrightness:`, values are clamped to the
/// documented 0.0-1.0 range by the setter below.
fn clamp_brightness(value: CGFloat) -> CGFloat {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIScreen: NSObject

// MARK: - Singleton

+ (id)mainScreen {
    if let Some(screen) = env.framework_state.uikit.ui_screen.main_screen {
        screen
    } else {
        let new = env.objc.alloc_static_object(
            this,
            Box::new(TrivialHostObject),
            &mut env.mem,
        );
        env.framework_state.uikit.ui_screen.main_screen = Some(new);
        new
    }
}

+ (id)screens {
    // Only one screen is ever available.
    let main: id = msg![env; this mainScreen];
    msg_class![env; NSArray arrayWithObject:main]
}

// MARK: - Retain / release (singleton — no-ops)

- (id)retain      { this }
- (())release     {}
- (id)autorelease { this }

// MARK: - Geometry

- (CGRect)bounds {
    let (width, height) = screen_size_for_current_orientation(env);
    if crate::env_flag_cached!("TOUCHHLE_LANDSCAPE_UISCREEN_BOUNDS") {
        log!(
            "TOUCHHLE_LANDSCAPE_UISCREEN_BOUNDS=1: UIScreen bounds reporting {}x{}",
            width,
            height
        );
    }
    CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width:  width  as CGFloat,
            height: height as CGFloat,
        },
    }
}

- (CGRect)nativeBounds {
    // Same as bounds at scale 1 — we don't model the physical pixel grid.
    msg![env; this bounds]
}

- (CGRect)applicationFrame {
    let mut bounds: CGRect = msg![env; this bounds];
    const STATUS_BAR_HEIGHT: CGFloat = 20.0;
    if !env.framework_state.uikit.ui_application.status_bar_hidden {
        bounds.origin.y    += STATUS_BAR_HEIGHT;
        bounds.size.height -= STATUS_BAR_HEIGHT;
    }
    bounds
}

// MARK: - Scale

- (CGFloat)scale {
    env.window().screen_scale() as CGFloat
}

- (CGFloat)nativeScale {
    // On real iOS devices nativeScale equals scale (the point-to-pixel
    // ratio of the physical display). Games use it to detect Retina and
    // size their framebuffers; reporting 1.0 while `scale` reports 2.0
    // made such games allocate a half-resolution framebuffer and draw
    // zoomed / cropped.
    env.window().screen_scale() as CGFloat
}

// MARK: - Brightness

- (CGFloat)brightness {
    env.framework_state.uikit.ui_screen.brightness
}

- (())setBrightness:(CGFloat)brightness {
    // Per Apple documentation, brightness is in the range 0.0 (darkest)
    // to 1.0 (lightest); clamp out-of-range values like the real UIKit.
    let clamped = clamp_brightness(brightness);
    if clamped != env.framework_state.uikit.ui_screen.brightness {
        log!(
            "[UIScreen setBrightness:] {} -> {} (host display unaffected)",
            env.framework_state.uikit.ui_screen.brightness,
            clamped
        );
        env.framework_state.uikit.ui_screen.brightness = clamped;
    }
}

- (bool)wantsSoftwareDimming {
    env.framework_state.uikit.ui_screen.wants_software_dimming
}

- (())setWantsSoftwareDimming:(bool)value {
    // Per Apple documentation this only selects whether brightness changes
    // are implemented by dimming; store the preference.
    env.framework_state.uikit.ui_screen.wants_software_dimming = value;
}

// MARK: - Display mode / overscan

- (id)currentMode {
    let (width, height) = screen_size_for_current_orientation(env);
    let size = CGSize {
        width:  width  as CGFloat,
        height: height as CGFloat,
    };

    crate::frameworks::uikit::ui_screen_mode::from_size(env, size, 1.0)
}

- (id)preferredMode {
    nil
}

- (id)availableModes {
    let current_mode: id = msg![env; this currentMode];
    msg_class![env; NSArray arrayWithObject:current_mode]
}

// Apple's
// <https://developer.apple.com/documentation/uikit/uiscreen/1617815-setcurrentmode>
// (now deprecated, was originally on UIScreen but in iOS 5+ Apple moved
// resolution control to UIScreenMode itself). The setter is documented as
// a no-op on screens that have only a single supported mode, which is
// the case for every device we emulate (a single fixed framebuffer).
// Accept the call so apps that probe / try to set the mode at launch
// don't get a "doesNotRecognizeSelector" log spam.
- (())setCurrentMode:(id)_mode {
    log_dbg!("[UIScreen setCurrentMode:] ignored; we emulate a single fixed display mode.");
}

- (CGFloat)overscanCompensationInsets {
    // UIEdgeInsetsZero as four floats would need a custom return type.
    // Return 0.0 as a stand-in; the real return type is UIEdgeInsets.
    0.0
}

// MARK: - Mirroring (iOS 4.3+)

- (id)mirroredScreen {
    nil
}

- (bool)isCaptured {
    false
}

// MARK: - Coordinate conversion helpers

- (CGRect)convertRect:(CGRect)rect toScreen:(id)_other_screen {
    // Single-screen device — coordinates are always the same.
    rect
}

- (CGRect)convertRect:(CGRect)rect fromScreen:(id)_other_screen {
    rect
}

- (CGPoint)convertPoint:(CGPoint)point toScreen:(id)_other_screen {
    point
}

- (CGPoint)convertPoint:(CGPoint)point fromScreen:(id)_other_screen {
    point
}
// MARK: - Display Link

- (id)displayLinkWithTarget:(id)target selector:(SEL)selector {
    msg_class![env; CADisplayLink displayLinkWithTarget:target selector:selector]
}

@end

};
