//! App picker GUI.

use crate::bundle::Bundle;
use crate::frameworks::core_graphics::cg_bitmap_context::{
    CGBitmapContextCreate, CGBitmapContextCreateImage,
};
use crate::frameworks::core_graphics::cg_color_space::CGColorSpaceCreateDeviceRGB;
use crate::frameworks::core_graphics::cg_context::{
    CGContextFillRect, CGContextRelease, CGContextRestoreGState, CGContextSaveGState,
    CGContextScaleCTM, CGContextSetRGBFillColor, CGContextTranslateCTM,
};
use crate::frameworks::core_graphics::cg_image::{self, kCGImageAlphaPremultipliedLast};
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_run_loop::run_run_loop_single_iteration;
use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::NSInteger;
use crate::frameworks::uikit::ui_font::{
    UITextAlignmentCenter, UITextAlignmentLeft, UITextAlignmentRight,
};
use crate::frameworks::uikit::ui_graphics::{UIGraphicsPopContext, UIGraphicsPushContext};
use crate::frameworks::uikit::ui_view::ui_control::ui_button::{
    UIButtonTypeCustom, UIButtonTypeRoundedRect,
};
use crate::frameworks::uikit::ui_view::ui_control::{
    UIControlEventTouchUpInside, UIControlEventValueChanged, UIControlStateNormal,
};
use crate::fs::BundleData;
use crate::image::Image;
use crate::mem::Ptr;
use crate::objc::{id, msg, msg_class, nil, objc_classes, release, ClassExports, HostObject};
use crate::options::Options;
use crate::paths;
use crate::window::DeviceOrientation;
use crate::Environment;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct AppInfo {
    path: PathBuf,
    display_name: String,
    icon: Option<Image>,
    /// `NSString*`
    display_name_ns_string: Option<id>,
    /// `UIImage*`
    icon_ui_image: Option<id>,
}

pub fn app_picker(options: Options) -> Result<(PathBuf, Vec<String>), String> {
    let apps_dir = paths::user_data_base_path().join(paths::APPS_DIR);

    let apps: Result<Vec<AppInfo>, String> = if !apps_dir.is_dir() {
        Err(format!("The {} directory couldn't be found. Check you're running touchHLE from the right directory.", apps_dir.display()))
    } else {
        enumerate_apps(&apps_dir).map_err(|err| {
            format!(
                "Couldn't get list of apps in the {} directory: {}.",
                apps_dir.display(),
                err
            )
        })
    };

    show_app_picker_gui(options, apps)
}

fn enumerate_apps(apps_dir: &Path) -> Result<Vec<AppInfo>, std::io::Error> {
    let mut apps = Vec::new();
    let mut directories = vec![apps_dir.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let app_path = entry?.path();
            let extension = app_path.extension();
            if extension == Some(OsStr::new("app")) || extension == Some(OsStr::new("ipa")) {
                let (bundle, fs) = match BundleData::open_any(&app_path).and_then(|bundle_data| {
                    Bundle::new_bundle_and_fs_from_host_path(
                        bundle_data,
                        /* read_only_mode: */ true,
                    )
                }) {
                    Ok(ok) => ok,
                    Err(e) => {
                        log!(
                            "Warning: couldn't open app bundle {}: {} (skipping)",
                            app_path.display(),
                            e
                        );
                        continue;
                    }
                };

                let display_name = bundle.display_name().to_owned();
                let icon = match bundle.load_icon(&fs) {
                    Ok(icon) => Some(icon),
                    Err(e) => {
                        log!("Warning: couldn't load icon for app bundle {}: {} (displaying placeholder instead)", app_path.display(), e);
                        None
                    }
                };

                apps.push(AppInfo {
                    path: app_path,
                    display_name,
                    icon,
                    display_name_ns_string: None,
                    icon_ui_image: None,
                });
            } else if app_path.is_dir() {
                directories.push(app_path);
            }
        }
    }

    apps.sort_by_key(|app| app.display_name.to_uppercase());
    Ok(apps)
}

/// Watch state for detecting a newly copied-in .ipa file.
struct IpaWatch {
    last_seen: Vec<(String, u64)>,
    dirty: bool,
    last_change: Option<Instant>,
}

/// List the .ipa files directly inside the apps directory, as (name, size).
///
/// The size matters: a file that is still being copied grows, and must not
/// be opened until it is complete. This is cheap enough to poll every
/// run-loop iteration, unlike a full [enumerate_apps], which opens every
/// app bundle.
fn list_top_level_ipa_files(apps_dir: &Path) -> Vec<(String, u64)> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(apps_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension() == Some(OsStr::new("ipa")) {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((name, size));
            }
        }
    }
    files.sort();
    files
}

/// How long the .ipa listing must stay unchanged before the app grid is
/// refreshed, so that a file that is still being copied is left alone.
const IPA_COPY_SETTLE_TIME: Duration = Duration::from_millis(500);

#[derive(Default)]
struct AppPickerDelegateHostObject {
    icon_tapped: id,
    // Set by the add-app tile; kept separately from icon_tapped because the
    // picker needs to wait for a newly copied IPA to settle before reloading.
    add_ipa: bool,
    quick_options_show: bool,
    quick_options_hide: bool,
    scale_hack_default: bool,
    scale_hack1: bool,
    scale_hack2: bool,
    scale_hack3: bool,
    scale_hack4: bool,
    orientation_default: bool,
    orientation_landscape_left: bool,
    orientation_landscape_right: bool,
    orientation_portrait_upside_down: bool,
    analog_stick_tilt_controls: Option<bool>,
    network: Option<bool>,
    show_fps: Option<bool>,
    cheat_engine: Option<bool>,
    trace_gl_errors: Option<bool>,
    force_composition: Option<bool>,
    gles_native: Option<bool>,
    fullscreen: Option<bool>,
    device_model_tag: Option<i32>,
    device_model_toggle: bool,
    device_model_scroll_up: bool,
    device_model_scroll_down: bool,
}
impl HostObject for AppPickerDelegateHostObject {}

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    // Not a real iOS dylib obviously. This shouldn't really be in the list of
    // dylibs if we can avoid it somehow (TODO?).
    path: "/.touchHLE/AppPickerHelpers.dylib",
    aliases: &[],
    class_exports: &[CLASSES],
    constant_exports: &[],
    function_exports: &[],
};

/// Be careful! These classes go in the normal class list, just like everything
/// else, so an app could try to instantiate them. Don't give them special
/// powers that could be exploited!
const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_AppPickerDelegate: NSObject

- (())iconTapped:(id)sender {
    // There is no allocWithZone: that creates AppPickerDelegateHostObject, so
    // this downcast effectively acts as an assertion that this class is being
    // used within the app picker, so it can't be abused. :)
    let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);
    host_obj.icon_tapped = sender;
}

- (())addIpa {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).add_ipa = true;
}

- (())quickOptionsShow {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_show = true;
}
- (())quickOptionsHide {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_hide = true;
}
- (())scaleHackDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack_default = true;
}
- (())scaleHack1 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack1 = true;
}
- (())scaleHack2 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack2 = true;
}
- (())scaleHack3 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack3 = true;
}
- (())scaleHack4 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack4 = true;
}
- (())orientationDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_default = true;
}
- (())orientationLandscapeLeft {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_landscape_left = true;
}
- (())orientationLandscapeRight {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_landscape_right = true;
}
- (())orientationPortraitUpsideDown {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_portrait_upside_down = true;
}
- (())analogStickTiltControls:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).analog_stick_tilt_controls = Some(switch_state);
}
- (())network:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).network = Some(switch_state);
}
- (())traceGLErrors:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).trace_gl_errors = Some(switch_state);
}
- (())forceComposition:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).force_composition = Some(switch_state);
}
- (())glesNative:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).gles_native = Some(switch_state);
}
- (())showFPS:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).show_fps = Some(switch_state);
    // Immediately reflect the runtime change so users see the overlay without
    // having to re-launch or wait for the option to be applied.
    // SAFETY: calling into the runtime-level API is safe from the UI thread.
    if switch_state {
        std::env::set_var("TOUCHHLE_ONSCREEN_FPS", "1");
        crate::gles::present::set_onscreen_fps_enabled(true);
    } else {
        std::env::remove_var("TOUCHHLE_ONSCREEN_FPS");
        crate::gles::present::set_onscreen_fps_enabled(false);
    }
}
- (())cheatEngine:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).cheat_engine = Some(switch_state);
}
- (())fullscreen:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).fullscreen = Some(switch_state);
}
- (())deviceModel:(id)sender { // UIButton*
    let tag: NSInteger = msg![env; sender tag];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).device_model_tag = Some(tag as i32);
}
- (())deviceModelToggle {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).device_model_toggle = true;
}
- (())deviceModelScrollUp {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).device_model_scroll_up = true;
}
- (())deviceModelScrollDown {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).device_model_scroll_down = true;
}
- (())openFileManager {
    // Assert (see above).
    let _ = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);

    match paths::url_for_opening_apps_dir() {
        Ok(url) => {
            // Our `openURL:` implementation is bypassed because it doesn't
            // allow non-web URLs.
            let url_res = crate::window::open_url(env, &url);
            if let Err(e) = url_res {
                echo!("Couldn't open file manager at {:?}: {}", url, e);
            } else {
                echo!("Opened game folder at {:?}, returning to the picker.", url);
            }
        },
        Err(e) => echo!("Couldn't open file manager: {}", e),
    }
}

@end

};

fn show_app_picker_gui(
    options: Options,
    apps: Result<Vec<AppInfo>, String>,
) -> Result<(PathBuf, Vec<String>), String> {
    let icon = {
        let bytes: &[u8] = match crate::branding() {
            "" => include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icon.png")),
            "UNOFFICIAL" => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/res/icon_unofficial.png"
            )),
            "PREVIEW" => {
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icon_preview.png"))
            }
            _ => panic!(),
        };
        let mut image = Image::from_bytes(bytes).unwrap();
        // should match Bundle::load_icon()
        // Use a slightly smaller corner radius for larger icons for a cleaner look.
        let corner_radius_px = 12.0;
        image.round_corners(
            corner_radius_px,
            /* four_corners: */ true,
            /* add_sheen: */ true,
        );
        image
    };
    let environment = Environment::new_without_app(options, icon)?;
    Ok(environment.run_app_picker(|env| app_picker_inner(env, apps)))
}

const APP_LAUNCH_ANIMATION_DURATION: Duration = Duration::from_millis(460);

/// A symmetric ease-in/ease-out curve gives the icon a deliberate, old-iOS
/// style zoom instead of the abrupt snap used by the previous picker.
fn app_launch_animation_eased_progress(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}

fn interpolate_rect(from: CGRect, to: CGRect, progress: CGFloat) -> CGRect {
    let interpolate = |from: CGFloat, to: CGFloat| from + (to - from) * progress;
    CGRect {
        origin: CGPoint {
            x: interpolate(from.origin.x, to.origin.x),
            y: interpolate(from.origin.y, to.origin.y),
        },
        size: CGSize {
            width: interpolate(from.size.width, to.size.width),
            height: interpolate(from.size.height, to.size.height),
        },
    }
}

/// Zoom the tapped icon to the size of the display while fading away the
/// picker beneath it. The picker is about to be discarded, so a temporary
/// UIImageView is enough to preserve the exact icon shown in the grid.
fn animate_app_launch_transition(
    env: &mut Environment,
    main_run_loop: id,
    window: id,
    main_view: id,
    icon_button: id,
) {
    let icon_image_view: id = msg![env; icon_button imageView];
    let icon_image: id = msg![env; icon_image_view image];
    if icon_image == nil {
        // The app picker normally supplies even missing app icons with a
        // placeholder. Keep launch working if a custom button has no image.
        return;
    }

    let icon_bounds: CGRect = msg![env; icon_image_view bounds];
    let from_frame: CGRect = msg![env; icon_image_view convertRect:icon_bounds toView:window];
    let to_frame: CGRect = msg![env; window bounds];

    let launch_image_view: id = msg_class![env; UIImageView alloc];
    let launch_image_view: id = msg![env; launch_image_view initWithImage:icon_image];
    () = msg![env; launch_image_view setFrame:from_frame];
    () = msg![env; launch_image_view setUserInteractionEnabled:false];
    () = msg![env; window addSubview:launch_image_view];

    let start_time = Instant::now();
    loop {
        let elapsed = start_time.elapsed();
        let progress = app_launch_animation_eased_progress(
            elapsed.as_secs_f64() / APP_LAUNCH_ANIMATION_DURATION.as_secs_f64(),
        );
        let progress = progress as CGFloat;
        let frame = interpolate_rect(from_frame, to_frame, progress);
        () = msg![env; launch_image_view setFrame:frame];
        () = msg![env; main_view setAlpha:(1.0 - progress)];

        // Force a frame so every interpolated position is presented instead
        // of waiting for the picker's regular display-link cadence.
        let _ = crate::frameworks::core_animation::recomposite_if_necessary(
            env,
            /* force: */ true,
        );

        if elapsed >= APP_LAUNCH_ANIMATION_DURATION {
            break;
        }
        run_run_loop_single_iteration(env, main_run_loop);
    }
}

fn app_picker_inner(
    env: &mut Environment,
    mut apps: Result<Vec<AppInfo>, String>,
) -> (PathBuf, Vec<String>) {
    let mut option_args = Vec::new();
    // Note that objects are generally not released in this code, because they
    // don't need to be: the entire Environment is thrown away at the end.

    // Bypassing UIApplicationMain!
    let ui_application: id = msg_class![env; UIApplication new];
    let delegate = env
        .objc
        .get_known_class("_touchHLE_AppPickerDelegate", &mut env.mem);
    let delegate = env.objc.alloc_object(
        delegate,
        Box::<AppPickerDelegateHostObject>::default(),
        &mut env.mem,
    );
    () = msg![env; ui_application setDelegate:delegate];

    let screen: id = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];

    let window: id = msg_class![env; UIWindow alloc];
    let window: id = msg![env; window initWithFrame:bounds];

    let app_frame: CGRect = msg![env; screen applicationFrame];
    let main_view: id = msg_class![env; UIView alloc];
    let main_view: id = msg![env; main_view initWithFrame:app_frame];
    () = msg![env; window addSubview:main_view];

    // Wallpaper
    let mut found_wallpaper = false;
    let mut have_wallpaper = false;
    for candidate in paths::WALLPAPER_FILES {
        let candidate = paths::user_data_base_path().join(candidate);
        if !candidate.exists() {
            continue;
        }
        found_wallpaper = true;

        let image = match std::fs::read(&candidate) {
            Ok(image) => image,
            Err(e) => {
                log!("Warning: couldn't read {}: {}", candidate.display(), e);
                break;
            }
        };
        let image = match Image::from_bytes(&image) {
            Ok(image) => image,
            Err(e) => {
                log!("Warning: couldn't decode {}: {}", candidate.display(), e);
                break;
            }
        };

        let image = cg_image::from_image(env, image);
        let image: id = msg_class![env; UIImage imageWithCGImage:image];
        let wallpaper: id = msg_class![env; UIImageView alloc];
        let wallpaper: id = msg![env; wallpaper initWithImage:image];
        () = msg![env; wallpaper setFrame:(CGRect {
            origin: CGPoint {
                x: 0.0,
                y: 0.0,
            },
            size: app_frame.size,
        })];
        () = msg![env; wallpaper setAlpha:(0.5 as CGFloat)];
        () = msg![env; main_view addSubview:wallpaper];
        have_wallpaper = true;
        break;
    }
    if !found_wallpaper {
        let CGSize { width, height } = app_frame.size;
        log!(
            "No wallpaper found; filename can be one of: {}; ideal size is {}×{} pixels",
            paths::WALLPAPER_FILES.join(", "),
            width,
            height,
        );
    }

    // Build label
    {
        let label_frame = CGRect {
            origin: CGPoint {
                x: 0.0,
                y: app_picker_version_label_top(app_frame.size.height),
            },
            size: CGSize {
                width: app_frame.size.width - 5.0,
                height: APP_PICKER_VERSION_LABEL_HEIGHT,
            },
        };
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:label_frame];
        let text = ns_string::from_rust_string(
            env,
            format!("{HYPERHLE_FORK_NAME} ({})", crate::COMMIT_HASH),
        );
        () = msg![env; label setText:text];
        () = msg![env; label setTextAlignment:UITextAlignmentRight];
        let font_size: CGFloat = 12.0;
        let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
        () = msg![env; label setFont:font];
        let text_color: id = if have_wallpaper {
            msg_class![env; UIColor whiteColor]
        } else {
            msg_class![env; UIColor lightGrayColor]
        };
        () = msg![env; label setTextColor:text_color];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; label setBackgroundColor:bg_color];
        () = msg![env; main_view addSubview:label];
    }

    let brand_color: id = if crate::branding() == "UNOFFICIAL" {
        msg_class![env; UIColor redColor]
    } else {
        msg_class![env; UIColor grayColor]
    };

    let title_frame = CGRect {
        origin: CGPoint { x: 12.0, y: 4.0 },
        size: CGSize {
            width: app_frame.size.width - 24.0,
            height: 34.0,
        },
    };
    let title: id = msg_class![env; UILabel alloc];
    let title: id = msg![env; title initWithFrame:title_frame];
    let text = ns_string::from_rust_string(env, HYPERHLE_FORK_NAME.to_string());
    () = msg![env; title setText:text];
    () = msg![env; title setTextAlignment:UITextAlignmentCenter];
    let font_size: CGFloat = 28.0;
    let font: id = msg_class![env; UIFont boldSystemFontOfSize:font_size];
    () = msg![env; title setFont:font];
    () = msg![env; title setTextColor:brand_color];
    let bg_color: id = msg_class![env; UIColor clearColor];
    () = msg![env; title setBackgroundColor:bg_color];
    () = msg![env; main_view addSubview:title];

    let quick_options_button_top = app_picker_quick_options_button_top(app_frame.size.height);

    let mut icon_grid_stuff = match &mut apps {
        Ok(ref mut apps) => {
            let mut icon_grid_stuff = make_icon_grid(
                env,
                delegate,
                main_view,
                app_frame,
                apps.len(),
                have_wallpaper,
            );
            update_icon_grid(env, &mut icon_grid_stuff, apps, 0);
            Some(icon_grid_stuff)
        }
        Err(e) => {
            let label_frame = CGRect {
                origin: CGPoint { x: 10.0, y: 10.0 },
                size: CGSize {
                    width: app_frame.size.width - 20.0,
                    height: quick_options_button_top - 20.0,
                },
            };
            let label: id = msg_class![env; UILabel alloc];
            let label: id = msg![env; label initWithFrame:label_frame];
            let text = ns_string::from_rust_string(env, e.clone());
            () = msg![env; label setText:text];
            () = msg![env; label setTextAlignment:UITextAlignmentCenter];
            () = msg![env; label setNumberOfLines:0]; // unlimited
            let text_color: id = msg_class![env; UIColor lightGrayColor];
            () = msg![env; label setTextColor:text_color];
            let bg_color: id = msg_class![env; UIColor clearColor];
            () = msg![env; label setBackgroundColor:bg_color];
            () = msg![env; main_view addSubview:label];
            None
        }
    };

    // Keep the sole footer action directly above the build label, leaving the
    // space above it available for a fourth row of app icons.
    let buttons_row_center = app_picker_quick_options_button_center(app_frame.size.height);
    make_button_row(
        env,
        delegate,
        main_view,
        app_frame.size,
        buttons_row_center,
        &[("Quick options", "quickOptionsShow")],
        None,
    );

    let mut quick_options_cheat_engine = quick_options_trainer_enabled(&env.options);
    let angle_backend_available = crate::window::angle_backend_available();
    let (mut quick_options_gles_native, quick_options_gles_native_switch_enabled) =
        quick_options_gles_native_state(&env.options, angle_backend_available);
    let quick_options_force_composition_enabled = env.options.force_composition;
    let quick_options_stuff = setup_quick_options(
        env,
        delegate,
        main_view,
        app_frame,
        quick_options_cheat_engine,
        quick_options_gles_native,
        quick_options_gles_native_switch_enabled,
        quick_options_force_composition_enabled,
    );
    let mut quick_options_scale_hack: Option<NonZeroU32> = None;
    let mut quick_options_fullscreen: Option<()> = None;
    let mut quick_options_orientation: Option<DeviceOrientation> = None;
    let mut quick_options_analog_stick_tilt_controls = true;
    let mut quick_options_network = false;
    let mut quick_options_show_fps = false;
    let mut quick_options_trace_gl_errors = false;
    let mut quick_options_force_composition_override: Option<bool> = None;
    let mut quick_options_device_tag: Option<i32> = None;
    let mut quick_options_device_model_open = false;
    let mut quick_options_device_model_scroll: isize = 0;

    fn update_quick_option_buttons(env: &mut Environment, buttons: &[id], selected_idx: usize) {
        for (idx, &button) in buttons.iter().enumerate() {
            let color: id = if idx == selected_idx {
                msg_class![env; UIColor magentaColor]
            } else {
                msg_class![env; UIColor grayColor]
            };
            () = msg![env; button setBackgroundColor:color];
        }
    }
    fn update_scale_hack_buttons(env: &mut Environment, buttons: &[id], value: Option<NonZeroU32>) {
        update_quick_option_buttons(env, buttons, value.map_or(0, |v| v.get() as usize));
    }
    fn update_orientation_buttons(
        env: &mut Environment,
        buttons: &[id],
        value: Option<DeviceOrientation>,
    ) {
        update_quick_option_buttons(
            env,
            buttons,
            value.map_or(0, |v| match v {
                DeviceOrientation::LandscapeLeft => 1,
                DeviceOrientation::LandscapeRight => 2,
                DeviceOrientation::PortraitUpsideDown => 3,
                _ => panic!(),
            }),
        );
    }
    update_scale_hack_buttons(
        env,
        &quick_options_stuff.scale_hack_buttons,
        quick_options_scale_hack,
    );
    update_orientation_buttons(
        env,
        &quick_options_stuff.orientation_buttons,
        quick_options_orientation,
    );
    update_device_model_menu(
        env,
        &quick_options_stuff.device_model_items,
        quick_options_stuff.device_model_thumb,
        quick_options_device_tag,
        quick_options_device_model_scroll,
    );

    () = msg![env; window makeKeyAndVisible];

    let apps_dir = paths::user_data_base_path().join(paths::APPS_DIR);
    let mut current_page = 0;
    // If the user taps the "+" tile, this records the .ipa files that existed
    // at that moment; once a new one shows up, the app list is re-enumerated.
    let mut awaited_ipa: Option<IpaWatch> = None;

    let main_run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
    // If an app is picked, this loop returns. If the user quits touchHLE, the
    // process exits.
    let app_path = loop {
        run_run_loop_single_iteration(env, main_run_loop);
        let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(delegate);
        let icon_tapped = std::mem::take(&mut host_obj.icon_tapped);
        if icon_tapped != nil {
            match icon_grid_stuff.as_ref().unwrap().icon_map.get(&icon_tapped) {
                Some(&TappedIcon::App(app_idx)) => {
                    // Match the classic iPhone OS launch transition: zoom the
                    // selected icon out to fill the display, then hand off to
                    // the game once the animation has finished.
                    animate_app_launch_transition(
                        env,
                        main_run_loop,
                        window,
                        main_view,
                        icon_tapped,
                    );

                    let app_path = &apps.as_ref().unwrap()[app_idx].path;
                    echo!("Picked: {}", app_path.display());
                    break app_path.clone();
                }
                Some(&TappedIcon::ChangePage(page_idx)) => {
                    current_page = page_idx;
                    update_icon_grid(
                        env,
                        icon_grid_stuff.as_mut().unwrap(),
                        apps.as_mut().unwrap(),
                        page_idx,
                    );
                }
                Some(&TappedIcon::AddIpa) => {
                    // Handled by the main loop body below (next iteration).
                    env.objc
                        .borrow_mut::<AppPickerDelegateHostObject>(delegate)
                        .add_ipa = true;
                }
                None => (), // Tapped on a black space
            }
            continue;
        }
        if std::mem::take(&mut host_obj.add_ipa) {
            // Snapshot the .ipa files that exist right now, so that when the
            // system file picker finishes, we can detect the new file and
            // refresh the app grid.
            awaited_ipa = Some(IpaWatch {
                last_seen: list_top_level_ipa_files(&apps_dir),
                dirty: false,
                last_change: None,
            });
            // MainActivity (Android) opens the system file picker and copies
            // the picked file into the apps directory. On other platforms,
            // the apps directory is opened in the file manager instead.
            if let Err(e) = crate::window::launch_ipa_picker(env) {
                echo!("Couldn't open IPA picker: {}", e);
            }
        } else if std::mem::take(&mut host_obj.quick_options_show) {
            () = msg![env; (quick_options_stuff.main_view) setHidden:false];
        } else if std::mem::take(&mut host_obj.quick_options_hide) {
            () = msg![env; (quick_options_stuff.main_view) setHidden:true];
        } else if std::mem::take(&mut host_obj.scale_hack_default) {
            quick_options_scale_hack = None;
            update_scale_hack_buttons(
                env,
                &quick_options_stuff.scale_hack_buttons,
                quick_options_scale_hack,
            );
        } else if std::mem::take(&mut host_obj.scale_hack1) {
            quick_options_scale_hack = Some(NonZeroU32::new(1).unwrap());
            update_scale_hack_buttons(
                env,
                &quick_options_stuff.scale_hack_buttons,
                quick_options_scale_hack,
            );
        } else if std::mem::take(&mut host_obj.scale_hack2) {
            quick_options_scale_hack = Some(NonZeroU32::new(2).unwrap());
            update_scale_hack_buttons(
                env,
                &quick_options_stuff.scale_hack_buttons,
                quick_options_scale_hack,
            );
        } else if std::mem::take(&mut host_obj.scale_hack3) {
            quick_options_scale_hack = Some(NonZeroU32::new(3).unwrap());
            update_scale_hack_buttons(
                env,
                &quick_options_stuff.scale_hack_buttons,
                quick_options_scale_hack,
            );
        } else if std::mem::take(&mut host_obj.scale_hack4) {
            quick_options_scale_hack = Some(NonZeroU32::new(4).unwrap());
            update_scale_hack_buttons(
                env,
                &quick_options_stuff.scale_hack_buttons,
                quick_options_scale_hack,
            );
        } else if std::mem::take(&mut host_obj.orientation_default) {
            quick_options_orientation = None;
            update_orientation_buttons(
                env,
                &quick_options_stuff.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_landscape_left) {
            quick_options_orientation = Some(DeviceOrientation::LandscapeLeft);
            update_orientation_buttons(
                env,
                &quick_options_stuff.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_landscape_right) {
            quick_options_orientation = Some(DeviceOrientation::LandscapeRight);
            update_orientation_buttons(
                env,
                &quick_options_stuff.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_portrait_upside_down) {
            quick_options_orientation = Some(DeviceOrientation::PortraitUpsideDown);
            update_orientation_buttons(
                env,
                &quick_options_stuff.orientation_buttons,
                quick_options_orientation,
            );
        } else if let Some(tag) = std::mem::take(&mut host_obj.device_model_tag) {
            quick_options_device_tag = Some(tag);
            quick_options_device_model_open = false;
            () = msg![env; (quick_options_stuff.device_model_menu) setHidden:true];
            update_device_model_menu(
                env,
                &quick_options_stuff.device_model_items,
                quick_options_stuff.device_model_thumb,
                quick_options_device_tag,
                quick_options_device_model_scroll,
            );
            let title = format!("{} ▼", device_model_label_for_tag(quick_options_device_tag));
            let title_ns = ns_string::from_rust_string(env, title);
            () = msg![env; (quick_options_stuff.device_model_btn)
                setTitle:title_ns forState:UIControlStateNormal];
            release(env, title_ns);
        } else if std::mem::take(&mut host_obj.device_model_toggle) {
            quick_options_device_model_open = !quick_options_device_model_open;
            () = msg![env; (quick_options_stuff.device_model_menu)
                setHidden:(!quick_options_device_model_open)];
            let arrow = if quick_options_device_model_open {
                "▲"
            } else {
                "▼"
            };
            let title = format!(
                "{} {}",
                device_model_label_for_tag(quick_options_device_tag),
                arrow
            );
            let title_ns = ns_string::from_rust_string(env, title);
            () = msg![env; (quick_options_stuff.device_model_btn)
                setTitle:title_ns forState:UIControlStateNormal];
            release(env, title_ns);
        } else if std::mem::take(&mut host_obj.device_model_scroll_up) {
            if quick_options_device_model_scroll > 0 {
                quick_options_device_model_scroll -= 1;
            }
            update_device_model_menu(
                env,
                &quick_options_stuff.device_model_items,
                quick_options_stuff.device_model_thumb,
                quick_options_device_tag,
                quick_options_device_model_scroll,
            );
        } else if std::mem::take(&mut host_obj.device_model_scroll_down) {
            let max_scroll = (quick_options_stuff.device_model_items.len() as isize)
                .saturating_sub(DEVICE_MENU_VISIBLE_ITEMS as isize);
            if quick_options_device_model_scroll < max_scroll {
                quick_options_device_model_scroll += 1;
            }
            update_device_model_menu(
                env,
                &quick_options_stuff.device_model_items,
                quick_options_stuff.device_model_thumb,
                quick_options_device_tag,
                quick_options_device_model_scroll,
            );
        } else if let Some(enabled) = std::mem::take(&mut host_obj.analog_stick_tilt_controls) {
            quick_options_analog_stick_tilt_controls = enabled;
        } else if let Some(enabled) = std::mem::take(&mut host_obj.network) {
            quick_options_network = enabled;
        } else if let Some(enabled) = std::mem::take(&mut host_obj.cheat_engine) {
            quick_options_cheat_engine = enabled;
        } else if let Some(enabled) = std::mem::take(&mut host_obj.show_fps) {
            quick_options_show_fps = enabled;
        } else if let Some(trace_gl_errors) = std::mem::take(&mut host_obj.trace_gl_errors) {
            quick_options_trace_gl_errors = trace_gl_errors;
        } else if let Some(force_composition) = std::mem::take(&mut host_obj.force_composition) {
            quick_options_force_composition_override = Some(force_composition);
        } else if let Some(gles_native) = std::mem::take(&mut host_obj.gles_native) {
            quick_options_gles_native = gles_native || !crate::window::angle_backend_available();
        } else if let Some(fullscreen) = std::mem::take(&mut host_obj.fullscreen) {
            quick_options_fullscreen = match fullscreen {
                false => None,
                true => Some(()),
            };
        }

        // Detect .ipa files copied in by the "+" tile flow and refresh the
        // grid once the new file has finished copying (its size stops
        // changing and has stayed stable for a moment).
        if let Some(watch) = &mut awaited_ipa {
            let new_listing = list_top_level_ipa_files(&apps_dir);
            if new_listing != watch.last_seen {
                watch.last_seen = new_listing;
                watch.dirty = true;
                watch.last_change = Some(Instant::now());
            } else if watch.dirty
                && watch
                    .last_change
                    .is_some_and(|t| t.elapsed() >= IPA_COPY_SETTLE_TIME)
            {
                watch.dirty = false;
                if let Ok(new_apps) = enumerate_apps(&apps_dir) {
                    if let Some(grid) = icon_grid_stuff.as_mut() {
                        let mut new_apps = new_apps;
                        grid.pages =
                            compute_pages(grid.icon_buttons_and_labels.len(), new_apps.len());
                        if current_page >= grid.pages.len() {
                            current_page = grid.pages.len() - 1;
                        }
                        update_icon_grid(env, grid, &mut new_apps, current_page);
                        apps = Ok(new_apps);
                    }
                }
            }
        }
    };

    // Apply user-specified overrides
    if let Some(scale_hack) = quick_options_scale_hack {
        option_args.push(format!("--scale-hack={}", scale_hack.get()));
    }
    if let Some(orientation) = quick_options_orientation {
        option_args.push(
            match orientation {
                DeviceOrientation::LandscapeLeft => "--landscape-left",
                DeviceOrientation::LandscapeRight => "--landscape-right",
                DeviceOrientation::PortraitUpsideDown => "--upside-down",
                _ => todo!(),
            }
            .to_string(),
        );
    }
    if let Some(()) = quick_options_fullscreen {
        option_args.push("--fullscreen".to_string());
    }
    if !quick_options_analog_stick_tilt_controls {
        option_args.push("--disable-analog-stick-tilt-controls".to_string());
    }
    if quick_options_network {
        option_args.push("--allow-network-access".to_string());
    }
    option_args.push(quick_options_trainer_argument(quick_options_cheat_engine).to_string());

    if quick_options_show_fps {
        // Reuse existing CLI flag to enable FPS logging/counter behaviour.
        option_args.push("--print-fps".to_string());
        // Also enable the on-screen FPS overlay both via env var and runtime
        // flag so users don't need to set env vars manually.
        std::env::set_var("TOUCHHLE_ONSCREEN_FPS", "1");
        crate::gles::present::set_onscreen_fps_enabled(true);
    }

    if let Some(enabled) = quick_options_force_composition_override {
        option_args.push(quick_options_force_composition_argument(enabled).to_string());
    }

    if quick_options_trace_gl_errors {
        option_args.push("--trace-gl-errors".to_string());
    }
    // Always passed explicitly (like `--trainer`/`--no-trainer`): the base
    // default is now OFF, so the switch has to be able to turn the native
    // driver back on, and the picker's choice should win over any
    // `--gles-native`/`--no-gles-native` in the options files.
    option_args.push(quick_options_gles_native_argument(quick_options_gles_native).to_string());

    if let Some(tag) = quick_options_device_tag {
        let tag = tag as NSInteger;
        if tag == DEVICE_TAG_DEFAULT {
            // No override — fall back to the app bundle / built-in default.
        } else if tag == DEVICE_TAG_AUTO {
            option_args.push("--device-family=auto".to_string());
        } else if let Some(family) = crate::window::DeviceFamily::ALL_SELECTABLE.get(tag as usize) {
            option_args.push(format!("--device-family={}", family.option_name()));
        }
    }

    // Return the environment so some parts of it can be salvaged.
    (app_path, option_args)
}

const HYPERHLE_FORK_NAME: &str = "HyperHLE-Fork";

const APP_PICKER_VERSION_LABEL_HEIGHT: CGFloat = 15.0;
const APP_PICKER_VERSION_LABEL_BOTTOM_INSET: CGFloat = 5.0;
const APP_PICKER_FOOTER_GAP: CGFloat = 4.0;
const APP_PICKER_BUTTON_ROW_HEIGHT: CGFloat = 30.0;
const APP_PICKER_GRID_TOP: CGFloat = 40.0;
const APP_PICKER_GRID_TO_BUTTON_GAP: CGFloat = 6.0;
const APP_PICKER_ICON_ROWS: usize = 4;

const ICON_SIZE: CGSize = CGSize {
    width: 76.0,
    height: 76.0,
};
const ICON_IMAGE_INSET: CGFloat = 6.0;
const ICON_LABEL_TOP_GAP: CGFloat = 1.0;
const ICON_ROW_GAP: CGFloat = 1.0;
const ICON_COLUMN_GAP: CGFloat = 16.0;

fn app_picker_version_label_top(app_height: CGFloat) -> CGFloat {
    app_height - APP_PICKER_VERSION_LABEL_HEIGHT - APP_PICKER_VERSION_LABEL_BOTTOM_INSET
}

fn app_picker_quick_options_button_center(app_height: CGFloat) -> CGFloat {
    app_picker_version_label_top(app_height)
        - APP_PICKER_FOOTER_GAP
        - APP_PICKER_BUTTON_ROW_HEIGHT / 2.0
}

fn app_picker_quick_options_button_top(app_height: CGFloat) -> CGFloat {
    app_picker_quick_options_button_center(app_height) - APP_PICKER_BUTTON_ROW_HEIGHT / 2.0
}

fn app_picker_icon_grid_num_rows(app_height: CGFloat, label_height: CGFloat) -> usize {
    let grid_bottom = app_picker_quick_options_button_top(app_height)
        - APP_PICKER_GRID_TO_BUTTON_GAP;
    let cell_content_height = ICON_SIZE.height + ICON_LABEL_TOP_GAP + label_height;
    let cell_step_y = cell_content_height + ICON_ROW_GAP;
    let available_height = (grid_bottom - APP_PICKER_GRID_TOP - cell_content_height).max(0.0);
    ((available_height / cell_step_y).floor() as usize + 1).clamp(1, APP_PICKER_ICON_ROWS)
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn classic_phone_picker_has_four_icon_rows() {
        // A visible status bar leaves `UIScreen.applicationFrame` at 320x460.
        assert_eq!(app_picker_icon_grid_num_rows(460.0, 12.0), APP_PICKER_ICON_ROWS);
    }
}

enum TappedIcon {
    App(usize),
    ChangePage(usize),
    AddIpa,
}

struct IconGridStuff {
    icon_buttons_and_labels: Vec<(id, id)>,
    placeholder_icon: Option<id>,
    prev_icon: Option<id>,
    next_icon: Option<id>,
    plus_icon: Option<id>,
    pages: Vec<std::ops::Range<usize>>,
    icon_map: HashMap<id, TappedIcon>,
}

fn make_icon_grid(
    env: &mut Environment,
    delegate: id,
    main_view: id,
    app_frame: CGRect,
    total_app_count: usize,
    have_wallpaper: bool,
) -> IconGridStuff {
    let num_cols = if app_frame.size.width >= 380.0 {
        4
    } else if app_frame.size.width >= 270.0 {
        3
    } else {
        2
    };
    let num_cols_f = num_cols as CGFloat;
    let label_size = CGSize {
        width: 80.0,
        height: 12.0,
    };
    let icon_gap_x = ICON_COLUMN_GAP;
    let icon_gap_y = ICON_LABEL_TOP_GAP + label_size.height + ICON_ROW_GAP;
    let grid_top = APP_PICKER_GRID_TOP;
    let num_rows = app_picker_icon_grid_num_rows(app_frame.size.height, label_size.height);
    let icon_grid_width = (ICON_SIZE.width * num_cols_f) + icon_gap_x * (num_cols_f - 1.0);
    let icon_grid_origin = CGPoint {
        x: (app_frame.size.width - icon_grid_width) / 2.0,
        y: grid_top,
    };

    let icon_tapped_sel = env.objc.lookup_selector("iconTapped:").unwrap();

    let mut icon_buttons_and_labels = Vec::new();

    for i in 0..(num_cols * num_rows) {
        let col = i % num_cols;
        let row = i / num_cols;

        // Rounding is needed here to avoid a blurry or offset image.
        let icon_frame = CGRect {
            origin: CGPoint {
                x: (icon_grid_origin.x + (col as CGFloat) * (ICON_SIZE.width + icon_gap_x)).round(),
                y: (icon_grid_origin.y + (row as CGFloat) * (ICON_SIZE.height + icon_gap_y))
                    .round(),
            },
            size: ICON_SIZE,
        };
        let icon_button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
        () = msg![env; icon_button setFrame:icon_frame];
        let image_view: id = msg![env; icon_button imageView];
        let bounds: CGRect = msg![env; icon_button bounds];
        let inset = ICON_IMAGE_INSET;
        () = msg![env; image_view setFrame:(CGRect {
            origin: CGPoint { x: inset, y: inset },
            size: CGSize {
                width: (bounds.size.width - inset * 2.0).max(1.0),
                height: (bounds.size.height - inset * 2.0).max(1.0),
            },
        })];
        let layer: id = msg![env; image_view layer];
        let gravity = ns_string::get_static_str(env, "resizeAspect");
        () = msg![env; layer setContentsGravity:gravity];
        () = msg![env; icon_button addTarget:delegate
                                      action:icon_tapped_sel
                            forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; main_view addSubview:icon_button];

        // Rounding is needed here to avoid blurry text.
        let label_frame = CGRect {
            origin: CGPoint {
                x: (icon_frame.origin.x - (label_size.width - ICON_SIZE.width) / 2.0).round(),
                y: (icon_frame.origin.y + ICON_SIZE.height + ICON_LABEL_TOP_GAP).round(),
            },
            size: label_size,
        };
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:label_frame];
        () = msg![env; label setTextAlignment:UITextAlignmentCenter];
        let font_size: CGFloat = label_size.height - 2.0;
        let font: id = if have_wallpaper {
            msg_class![env; UIFont systemFontOfSize:font_size]
        } else {
            msg_class![env; UIFont boldSystemFontOfSize:font_size]
        };
        () = msg![env; label setFont:font];
        let text_color: id = if have_wallpaper {
            msg_class![env; UIColor whiteColor]
        } else {
            msg_class![env; UIColor lightGrayColor]
        };
        () = msg![env; label setTextColor:text_color];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; label setBackgroundColor:bg_color];
        () = msg![env; main_view addSubview:label];

        icon_buttons_and_labels.push((icon_button, label));
    }

    // TODO: Use UIScrollView pagination and UIPageControl once available.
    let total_slots = icon_buttons_and_labels.len();
    let pages = compute_pages(total_slots, total_app_count);

    IconGridStuff {
        icon_buttons_and_labels,
        placeholder_icon: None,
        prev_icon: None,
        next_icon: None,
        plus_icon: None,
        pages,
        icon_map: HashMap::new(),
    }
}

/// Work out which apps go on each page of the icon grid.
///
/// Page 0 reserves its first slot for the "add IPA" (+) tile; the remaining
/// slots are used for the prev/next arrows (when relevant) and the apps.
fn compute_pages(total_slots: usize, total_app_count: usize) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    if total_app_count == 0 {
        pages.push(0..0);
        return pages;
    }
    let mut start = 0;
    while start < total_app_count {
        let page_idx = pages.len();
        let has_prev = start != 0;
        let has_plus = page_idx == 0;
        let mut app_slots = total_slots - usize::from(has_prev) - usize::from(has_plus);
        let remaining = total_app_count - start;
        if remaining > app_slots {
            app_slots -= 1;
        }
        let end = (start + app_slots).min(total_app_count);
        pages.push(start..end);
        start = end;
    }
    pages
}

fn make_icon_from_glyph(
    env: &mut Environment,
    glyph: char,
    font_size: CGFloat,
    baseline_offset: CGFloat,
    bg_color: (CGFloat, CGFloat, CGFloat, CGFloat),
) -> id {
    let ui_scale = env.options.ui_scale.get() as CGFloat;
    let color_space = CGColorSpaceCreateDeviceRGB(env);
    let context = CGBitmapContextCreate(
        env,
        Ptr::null(),
        (ICON_SIZE.width as u32).saturating_mul(env.options.ui_scale.get()),
        (ICON_SIZE.height as u32).saturating_mul(env.options.ui_scale.get()),
        8,
        4 * (ICON_SIZE.width as u32).saturating_mul(env.options.ui_scale.get()),
        color_space,
        kCGImageAlphaPremultipliedLast,
    );
    UIGraphicsPushContext(env, context);

    let scaled_width = ICON_SIZE.width * ui_scale;
    let scaled_height = ICON_SIZE.height * ui_scale;

    // Compensate for row order inversion. The offset is in bitmap pixels,
    // so it must be scaled along with everything else.
    CGContextSaveGState(env, context);
    CGContextTranslateCTM(env, context, 0.0, scaled_height);
    CGContextScaleCTM(env, context, ui_scale, -ui_scale);

    let (r, g, b, a) = bg_color;
    CGContextSetRGBFillColor(env, context, r, g, b, a);
    CGContextFillRect(
        env,
        context,
        CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: ICON_SIZE,
        },
    );
    CGContextRestoreGState(env, context);

    // Draw the glyph at 1:1 device pixels with a larger font, so it is
    // rasterized at the higher resolution instead of being upscaled by
    // the CTM (which would make it blurry).
    CGContextTranslateCTM(env, context, 0.0, scaled_height);
    CGContextScaleCTM(env, context, 1.0, -1.0);

    let font: id = msg_class![env; UIFont systemFontOfSize:(font_size * ui_scale)];
    let glyph_string: id = ns_string::from_rust_string(env, [glyph].into_iter().collect());
    let glyph_size: CGSize = msg![env; glyph_string sizeWithFont:font];
    CGContextSetRGBFillColor(env, context, 1.0, 1.0, 1.0, 1.0); // white
    let glyph_origin = CGPoint {
        x: scaled_width / 2.0 - glyph_size.width / 2.0,
        y: scaled_height / 2.0 - glyph_size.height / 2.0 + baseline_offset * ui_scale,
    };
    let _: CGSize = msg![env; glyph_string drawAtPoint:glyph_origin withFont:font];
    release(env, glyph_string);

    UIGraphicsPopContext(env);

    let cg_image = CGBitmapContextCreateImage(env, context);
    // This radius should match the one in src/bundle.rs, adjusted for the
    // bitmap resolution.
    cg_image::borrow_image_mut(&mut env.objc, cg_image).round_corners(
        12.0 * ui_scale,
        /* four_corners: */ true,
        /* add_sheen: */ true,
    );
    CGContextRelease(env, context);

    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);

    ui_image
}

fn update_icon_grid(
    env: &mut Environment,
    icon_grid_stuff: &mut IconGridStuff,
    apps: &mut [AppInfo],
    page_idx: usize,
) {
    icon_grid_stuff.icon_map.clear();

    let app_idx_range = icon_grid_stuff.pages[page_idx].clone();
    let have_prev_icon = page_idx != 0;
    let have_next_icon = app_idx_range.end != apps.len();

    let mut icon_iter = icon_grid_stuff.icon_buttons_and_labels.iter();

    if have_prev_icon {
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff.prev_icon.get_or_insert_with(|| {
            make_icon_from_glyph(env, '←', 50.0, -9.0, (0.25, 0.25, 0.25, 1.0))
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::ChangePage(page_idx - 1));
    }

    // The iOS-style "+" tile on the first page lets the user add a new app
    // by picking an .ipa file, which then gets copied into the apps folder.
    if page_idx == 0 {
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff.plus_icon.get_or_insert_with(|| {
            make_icon_from_glyph(env, '+', 50.0, -6.0, (0.25, 0.25, 0.25, 1.0))
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::AddIpa);
    }

    for app_idx in app_idx_range.clone() {
        let app = &mut apps[app_idx];

        let &(icon_button, label) = icon_iter.next().unwrap();

        if let Some(icon) = app.icon.take() {
            let image = cg_image::from_image(env, icon);
            let image: id = msg_class![env; UIImage imageWithCGImage:image];
            app.icon_ui_image = Some(image);
        }

        let image = app.icon_ui_image.unwrap_or_else(|| {
            *icon_grid_stuff.placeholder_icon.get_or_insert_with(|| {
                make_icon_from_glyph(env, '?', 40.0, 0.0, (0.5, 0.5, 0.5, 1.0))
            })
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];

        let text = *app
            .display_name_ns_string
            .get_or_insert_with(|| ns_string::from_rust_string(env, app.display_name.clone()));
        () = msg![env; label setText:text];

        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::App(app_idx));
    }

    if have_next_icon {
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff.next_icon.get_or_insert_with(|| {
            make_icon_from_glyph(env, '→', 50.0, -9.0, (0.25, 0.25, 0.25, 1.0))
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::ChangePage(page_idx + 1));
    }

    // There may be remaining spaces might need to be blanked.
    for &(icon_button, label) in icon_iter {
        () = msg![env; icon_button setImage:nil forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
    }
}

fn make_button_row(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    super_view_size: CGSize,
    buttons_row_center: CGFloat,
    buttons: &[(&'static str, &'static str)],
    font_size: Option<CGFloat>,
) -> Vec<id> {
    let margin = 10.0;

    let button_size = CGSize {
        width: (super_view_size.width - margin) / (buttons.len() as CGFloat) - margin,
        height: APP_PICKER_BUTTON_ROW_HEIGHT,
    };
    let mut button_frame = CGRect {
        origin: CGPoint {
            x: margin,
            y: buttons_row_center - button_size.height / 2.0,
        },
        size: button_size,
    };

    let mut ui_buttons = Vec::new();
    for (title_text, selector) in buttons {
        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
        let text = ns_string::get_static_str(env, title_text);
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:button_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];

        if let Some(font_size) = font_size {
            let label: id = msg![env; button titleLabel];
            let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
            () = msg![env; label setFont:font];
        }

        let selector = env.objc.lookup_selector(selector).unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; super_view addSubview:button];

        button_frame.origin.x += button_size.width + margin;
        ui_buttons.push(button);
    }
    ui_buttons
}

struct QuickOptionsStuff {
    main_view: id,
    scale_hack_buttons: [id; 5],
    orientation_buttons: [id; 4],
    /// The button that toggles the "Device model" dropdown open/closed. Its
    /// title shows the currently-selected model plus an up/down arrow.
    device_model_btn: id,
    /// The dropdown container view (hidden until toggled). Holds the scrollable
    /// list of choices, the scrollbar track/thumb, and the scroll arrows.
    device_model_menu: id,
    /// One button per choice in `device_model_entries()` order ("Default",
    /// "Auto", then every [crate::window::DeviceFamily] in `ALL_SELECTABLE`).
    /// Each carries a UIView `tag` identifying its choice (see
    /// `DEVICE_TAG_DEFAULT` / `DEVICE_TAG_AUTO` / model index).
    device_model_items: Vec<id>,
    /// The scrollbar thumb shown alongside the list.
    device_model_thumb: id,
}

/// Sentinel button tags for the device-model dropdown. Model buttons use their
/// index into `DeviceFamily::ALL_SELECTABLE` (0..=19) as their tag, so the
/// sentinels are placed well above that range.
const DEVICE_TAG_DEFAULT: NSInteger = 1000;
const DEVICE_TAG_AUTO: NSInteger = 1001;

/// How many rows of the device-model dropdown are visible at once before the
/// list has to be scrolled.
const DEVICE_MENU_VISIBLE_ITEMS: usize = 6;
/// Height of a single row in the device-model dropdown.
const DEVICE_MENU_ITEM_HEIGHT: CGFloat = 30.0;

/// The choices shown in the device-model dropdown, in display order, as
/// `(title, tag)` pairs: "Default" (no override), "Auto" (match host screen),
/// then one entry per [crate::window::DeviceFamily] in `ALL_SELECTABLE` order
/// tagged with its index.
fn device_model_entries() -> Vec<(String, NSInteger)> {
    use crate::window::DeviceFamily;
    let mut entries: Vec<(String, NSInteger)> = Vec::new();
    entries.push(("Default".to_string(), DEVICE_TAG_DEFAULT));
    entries.push(("Auto".to_string(), DEVICE_TAG_AUTO));
    for (idx, family) in DeviceFamily::ALL_SELECTABLE.iter().enumerate() {
        entries.push((family.display_name().to_string(), idx as NSInteger));
    }
    entries
}

/// Human-readable label for a device-model choice tag, used as the dropdown
/// button title.
fn device_model_label_for_tag(tag: Option<i32>) -> String {
    use crate::window::DeviceFamily;
    match tag.map(|t| t as NSInteger) {
        None | Some(DEVICE_TAG_DEFAULT) => "Default".to_string(),
        Some(DEVICE_TAG_AUTO) => "Auto".to_string(),
        Some(idx) => DeviceFamily::ALL_SELECTABLE
            .get(idx as usize)
            .map(|f| f.display_name().to_string())
            .unwrap_or_else(|| "Default".to_string()),
    }
}

fn setup_quick_options(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    app_frame: CGRect,
    cheat_engine_enabled: bool,
    gles_native_enabled: bool,
    gles_native_switch_enabled: bool,
    force_composition_enabled: bool,
) -> QuickOptionsStuff {
    // UIView*
    let main_frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: app_frame.size,
    };

    // Container for all the other stuff

    let main_view: id = msg_class![env; UIView alloc];
    let main_view: id = msg![env; main_view initWithFrame:main_frame];
    // TODO: Isn't white the default?
    let bg_color: id = msg_class![env; UIColor whiteColor];
    () = msg![env; main_view setBackgroundColor:bg_color];
    // This main_view is hidden until the quick options button is tapped.
    () = msg![env; main_view setHidden:true];
    () = msg![env; super_view addSubview:main_view];

    let divider = 44.0;

    // Close button (×) in the upper right corner. It uses an explicit border
    // and a slightly larger frame than the title so the glyph is clearly
    // visible against the white menu background.
    {
        let button_size: CGFloat = 36.0;
        let button_margin: CGFloat = 8.0;
        let button_frame = CGRect {
            origin: CGPoint {
                x: main_frame.size.width - button_size - button_margin,
                y: button_margin,
            },
            size: CGSize {
                width: button_size,
                height: button_size,
            },
        };

        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
        let text = ns_string::get_static_str(env, "×");
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:button_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];

        let label: id = msg![env; button titleLabel];
        let font: id = msg_class![env; UIFont systemFontOfSize:(28.0 as CGFloat)];
        () = msg![env; label setFont:font];

        // `buttonWithType:UIButtonTypeRoundedRect` does not actually apply the
        // rounded-rect appearance, so explicitly give the close button a
        // visible background, title color and rounded border. Without this
        // the white default title on a clear background would be invisible
        // against the white menu.
        let bg_color: id = msg_class![env; UIColor grayColor];
        () = msg![env; button setBackgroundColor:bg_color];
        let text_color: id = msg_class![env; UIColor whiteColor];
        () = msg![env; button setTitleColor:text_color forState:UIControlStateNormal];
        let layer: id = msg![env; button layer];
        () = msg![env; layer setCornerRadius:(8.0 as CGFloat)];

        let selector = env.objc.lookup_selector("quickOptionsHide").unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; main_view addSubview:button];
    }

    enum RowKind {
        Label(&'static str),
        Buttons(&'static [(&'static str, &'static str)], Option<CGFloat>),
        /// Dropdown listing every selectable device model.
        DeviceDropdown,
        Toggle(&'static str, &'static str, bool, bool),
    }
    let rows = [
        RowKind::Label("Scale hack"),
        RowKind::Buttons(
            &[
                ("Default", "scaleHackDefault"),
                ("Off", "scaleHack1"),
                ("2×", "scaleHack2"),
                ("3×", "scaleHack3"),
                ("4×", "scaleHack4"),
            ],
            Some(14.0),
        ),
        RowKind::Label("Orientation"),
        RowKind::Buttons(
            &[
                ("Default", "orientationDefault"),
                ("←", "orientationLandscapeLeft"),
                ("→", "orientationLandscapeRight"),
                ("↓", "orientationPortraitUpsideDown"),
            ],
            None,
        ),
        RowKind::Label("Device model"),
        RowKind::DeviceDropdown,
        RowKind::Toggle("Cheat Engine", "cheatEngine:", cheat_engine_enabled, true),
        RowKind::Toggle("Network access", "network:", false, true),
        RowKind::Toggle("Show FPS", "showFPS:", false, true),
        RowKind::Toggle("Trace GL errors", "traceGLErrors:", false, true),
        RowKind::Toggle(
            "GLES Native",
            "glesNative:",
            gles_native_enabled,
            gles_native_switch_enabled,
        ),
        RowKind::Toggle(
            "May fix graphics issues.",
            "forceComposition:",
            force_composition_enabled,
            true,
        ),
        RowKind::Toggle(
            "Use analog sticks for tilt controls",
            "analogStickTiltControls:",
            true,
            true,
        ),
        RowKind::Toggle("Fullscreen (override)", "fullscreen:", false, true),
    ];
    let rows = if crate::window::Window::rotatable_fullscreen() {
        // Fullscreen option doesn't make sense on always-fullscreen platforms
        &rows[..rows.len() - 1]
    } else {
        &rows[..]
    };

    let mut button_rows = Vec::new();
    let mut device_model_btn: id = nil;
    let mut device_model_menu: id = nil;
    let mut device_model_items: Vec<id> = Vec::new();
    let mut device_model_thumb: id = nil;
    for (i, row) in rows.iter().enumerate() {
        let row_center = divider
            + ((1 + i) as CGFloat)
                * ((main_frame.size.height - divider) / ((rows.len() + 1) as CGFloat));

        match *row {
            RowKind::Label(text) => {
                let frame = CGRect {
                    origin: CGPoint {
                        x: 0.0,
                        y: row_center - 30.0 / 2.0,
                    },
                    size: CGSize {
                        width: main_frame.size.width,
                        height: 30.0,
                    },
                };

                let label: id = msg_class![env; UILabel alloc];
                let label: id = msg![env; label initWithFrame:frame];
                let text = ns_string::get_static_str(env, text);
                () = msg![env; label setText:text];
                () = msg![env; label setTextAlignment:UITextAlignmentCenter];
                () = msg![env; main_view addSubview:label];
            }
            RowKind::Buttons(buttons, font_size) => {
                button_rows.push(make_button_row(
                    env,
                    delegate,
                    main_view,
                    main_frame.size,
                    row_center,
                    buttons,
                    font_size,
                ));
            }
            RowKind::DeviceDropdown => {
                let dropdown = make_device_model_dropdown(
                    env,
                    delegate,
                    main_view,
                    main_frame.size,
                    row_center,
                );
                device_model_btn = dropdown.0;
                device_model_menu = dropdown.1;
                device_model_items = dropdown.2;
                device_model_thumb = dropdown.3;
            }
            RowKind::Toggle(label_text, selector, default_state, enabled) => {
                let switch_width: CGFloat = 94.0;
                let switch_height: CGFloat = 27.0;
                let side_margin: CGFloat = 12.0;
                let label_gap: CGFloat = 8.0;
                let label_width = (main_frame.size.width
                    - switch_width
                    - side_margin * 2.0
                    - label_gap)
                    .max(0.0);
                let label_frame = CGRect {
                    origin: CGPoint {
                        x: side_margin,
                        y: row_center - 15.0,
                    },
                    size: CGSize {
                        width: label_width,
                        height: 30.0,
                    },
                };
                let label: id = msg_class![env; UILabel alloc];
                let label: id = msg![env; label initWithFrame:label_frame];
                let text = ns_string::get_static_str(env, label_text);
                () = msg![env; label setText:text];
                () = msg![env; label setTextAlignment:UITextAlignmentLeft];
                let font: id = msg_class![env; UIFont systemFontOfSize:(16.0 as CGFloat)];
                () = msg![env; label setFont:font];
                () = msg![env; label setAdjustsFontSizeToFitWidth:true];
                () = msg![env; label setMinimumFontSize:(12.0 as CGFloat)];
                () = msg![env; main_view addSubview:label];

                let switch_frame = CGRect {
                    origin: CGPoint {
                        x: main_frame.size.width - side_margin - switch_width,
                        y: row_center - switch_height / 2.0,
                    },
                    size: Default::default(),
                };
                let switch: id = msg_class![env; UISwitch alloc];
                let switch: id = msg![env; switch initWithFrame:switch_frame];
                () = msg![env; switch setOn:default_state];
                () = msg![env; switch setEnabled:enabled];
                let selector = env.objc.lookup_selector(selector).unwrap();
                () = msg![env; switch addTarget:delegate
                                         action:selector
                               forControlEvents:UIControlEventValueChanged];
                () = msg![env; main_view addSubview:switch];
            }
        }
    }

    QuickOptionsStuff {
        main_view,
        scale_hack_buttons: button_rows[0][..].try_into().unwrap(),
        orientation_buttons: button_rows[1][..].try_into().unwrap(),
        device_model_btn,
        device_model_menu,
        device_model_items,
        device_model_thumb,
    }
}

/// Re-lay-out and re-style the device-model dropdown list for the given scroll
/// offset and current selection. Items are positioned relative to `scroll`
/// (each row is `DEVICE_MENU_ITEM_HEIGHT` tall); rows outside the visible
/// window are hidden. The currently-selected item is highlighted in magenta,
/// the rest in dark gray. The scrollbar thumb is moved to reflect `scroll`.
fn update_device_model_menu(
    env: &mut Environment,
    items: &[id],
    thumb: id,
    selected: Option<i32>,
    scroll: isize,
) {
    let visible_menu_height = (DEVICE_MENU_VISIBLE_ITEMS as CGFloat) * DEVICE_MENU_ITEM_HEIGHT;
    let list_width: CGFloat = 256.0;
    let max_scroll = (items.len() as isize).saturating_sub(DEVICE_MENU_VISIBLE_ITEMS as isize);

    for (j, &item) in items.iter().enumerate() {
        let y_pos = ((j as isize - scroll) as CGFloat) * DEVICE_MENU_ITEM_HEIGHT;
        let is_visible = y_pos >= 0.0 && y_pos < visible_menu_height;
        () = msg![env; item setHidden:(!is_visible)];
        if is_visible {
            let item_frame = CGRect {
                origin: CGPoint { x: 0.0, y: y_pos },
                size: CGSize {
                    width: list_width,
                    height: DEVICE_MENU_ITEM_HEIGHT,
                },
            };
            () = msg![env; item setFrame:item_frame];
        }
        let tag: NSInteger = msg![env; item tag];
        let is_selected = selected.is_some_and(|v| v as NSInteger == tag);
        let color: id = if is_selected {
            msg_class![env; UIColor magentaColor]
        } else {
            msg_class![env; UIColor darkGrayColor]
        };
        () = msg![env; item setBackgroundColor:color];
    }

    // Position the scrollbar thumb proportionally to the scroll offset.
    let thumb_height: CGFloat = 54.0;
    let travel = (visible_menu_height - thumb_height).max(0.0);
    let thumb_y = if max_scroll > 0 {
        (scroll as CGFloat / max_scroll as CGFloat) * travel
    } else {
        0.0
    };
    let thumb_frame = CGRect {
        origin: CGPoint {
            x: list_width,
            y: thumb_y,
        },
        size: CGSize {
            width: 24.0,
            height: thumb_height,
        },
    };
    () = msg![env; thumb setFrame:thumb_frame];
}

/// Build the "Device model" dropdown: a toggle button whose title shows the
/// currently-selected model and an up/down arrow, plus a (initially hidden)
/// menu placed *above* the button so it never runs off the bottom of the
/// screen. The menu contains a vertically-scrollable list of every choice from
/// [device_model_entries], a scrollbar track + thumb, and transparent up/down
/// scroll arrows. Each list item is wired to the delegate's `deviceModel:`
/// selector and tagged with its choice; the arrows fire `deviceModelScrollUp` /
/// `deviceModelScrollDown`. Returns `(toggle button, menu view, item buttons,
/// scrollbar thumb)`.
fn make_device_model_dropdown(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    super_view_size: CGSize,
    row_center: CGFloat,
) -> (id, id, Vec<id>, id) {
    let btn_width: CGFloat = 280.0;
    let btn_height: CGFloat = 30.0;
    let list_width: CGFloat = 256.0;
    let scrollbar_width: CGFloat = 24.0;

    let btn_frame = CGRect {
        origin: CGPoint {
            x: super_view_size.width / 2.0 - btn_width / 2.0,
            y: row_center - btn_height / 2.0,
        },
        size: CGSize {
            width: btn_width,
            height: btn_height,
        },
    };

    let dark_gray: id = msg_class![env; UIColor darkGrayColor];

    // Bordered container for the toggle button (a darker frame behind a lighter
    // inner button), so it reads as a control on the white menu background.
    let border_view: id = msg_class![env; UIView alloc];
    let border_view: id = msg![env; border_view initWithFrame:btn_frame];
    () = msg![env; border_view setBackgroundColor:dark_gray];
    () = msg![env; super_view addSubview:border_view];

    let inner_frame = CGRect {
        origin: CGPoint { x: 2.0, y: 2.0 },
        size: CGSize {
            width: btn_frame.size.width - 4.0,
            height: btn_frame.size.height - 4.0,
        },
    };
    let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    let initial_title = format!("{} ^", device_model_label_for_tag(None));
    let text = ns_string::from_rust_string(env, initial_title);
    () = msg![env; button setTitle:text forState:UIControlStateNormal];
    release(env, text);
    let black: id = msg_class![env; UIColor blackColor];
    () = msg![env; button setTitleColor:black forState:UIControlStateNormal];
    let light_gray: id = msg_class![env; UIColor lightGrayColor];
    () = msg![env; button setBackgroundColor:light_gray];
    () = msg![env; button setFrame:inner_frame];
    () = msg![env; button layoutSubviews];
    let toggle_selector = env.objc.lookup_selector("deviceModelToggle").unwrap();
    () = msg![env; button addTarget:delegate
                             action:toggle_selector
                   forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; border_view addSubview:button];

    // The dropdown menu, placed directly above the toggle button. It is clipped
    // to its own bounds and hidden until the button is tapped.
    let visible_menu_height = (DEVICE_MENU_VISIBLE_ITEMS as CGFloat) * DEVICE_MENU_ITEM_HEIGHT;
    let menu_frame = CGRect {
        origin: CGPoint {
            x: btn_frame.origin.x,
            y: btn_frame.origin.y - visible_menu_height,
        },
        size: CGSize {
            width: btn_width,
            height: visible_menu_height,
        },
    };
    let menu_view: id = msg_class![env; UIView alloc];
    let menu_view: id = msg![env; menu_view initWithFrame:menu_frame];
    () = msg![env; menu_view setBackgroundColor:dark_gray];
    () = msg![env; menu_view setClipsToBounds:true];
    () = msg![env; menu_view setHidden:true];
    () = msg![env; super_view addSubview:menu_view];

    // List items: one button per choice. Items that fall outside the initially
    // visible window are hidden; scrolling reveals them (see
    // `update_device_model_menu`).
    let entries = device_model_entries();
    let item_selector = env.objc.lookup_selector("deviceModel:").unwrap();
    let white: id = msg_class![env; UIColor whiteColor];
    let mut items: Vec<id> = Vec::new();
    for (j, (title, tag)) in entries.into_iter().enumerate() {
        let y_pos = (j as CGFloat) * DEVICE_MENU_ITEM_HEIGHT;
        let item_frame = CGRect {
            origin: CGPoint { x: 0.0, y: y_pos },
            size: CGSize {
                width: list_width,
                height: DEVICE_MENU_ITEM_HEIGHT,
            },
        };
        let item_btn: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
        let text = ns_string::from_rust_string(env, title);
        () = msg![env; item_btn setTitle:text forState:UIControlStateNormal];
        release(env, text);
        () = msg![env; item_btn setTitleColor:white forState:UIControlStateNormal];
        () = msg![env; item_btn setFrame:item_frame];
        () = msg![env; item_btn layoutSubviews];
        () = msg![env; item_btn setTag:tag];
        if y_pos >= visible_menu_height {
            () = msg![env; item_btn setHidden:true];
        }
        () = msg![env; item_btn addTarget:delegate
                                   action:item_selector
                         forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; menu_view addSubview:item_btn];
        items.push(item_btn);
    }

    // Scrollbar track (full height) and thumb.
    let track_view: id = msg_class![env; UIView alloc];
    let track_frame = CGRect {
        origin: CGPoint {
            x: list_width,
            y: 0.0,
        },
        size: CGSize {
            width: scrollbar_width,
            height: visible_menu_height,
        },
    };
    let track_view: id = msg![env; track_view initWithFrame:track_frame];
    let black: id = msg_class![env; UIColor blackColor];
    () = msg![env; track_view setBackgroundColor:black];
    () = msg![env; menu_view addSubview:track_view];

    let thumb_view: id = msg_class![env; UIView alloc];
    let thumb_frame = CGRect {
        origin: CGPoint {
            x: list_width,
            y: 0.0,
        },
        size: CGSize {
            width: scrollbar_width,
            height: 54.0,
        },
    };
    let thumb_view: id = msg![env; thumb_view initWithFrame:thumb_frame];
    let light_gray: id = msg_class![env; UIColor lightGrayColor];
    () = msg![env; thumb_view setBackgroundColor:light_gray];
    () = msg![env; menu_view addSubview:thumb_view];

    // Transparent up/down halves over the scrollbar that scroll the list.
    let clear: id = msg_class![env; UIColor clearColor];
    let up_btn: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    let up_frame = CGRect {
        origin: CGPoint {
            x: list_width,
            y: 0.0,
        },
        size: CGSize {
            width: scrollbar_width,
            height: visible_menu_height / 2.0,
        },
    };
    () = msg![env; up_btn setFrame:up_frame];
    () = msg![env; up_btn setBackgroundColor:clear];
    () = msg![env; up_btn addTarget:delegate
                             action:(env.objc.lookup_selector("deviceModelScrollUp").unwrap())
                   forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; menu_view addSubview:up_btn];

    let down_btn: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    let down_frame = CGRect {
        origin: CGPoint {
            x: list_width,
            y: visible_menu_height / 2.0,
        },
        size: CGSize {
            width: scrollbar_width,
            height: visible_menu_height / 2.0,
        },
    };
    () = msg![env; down_btn setFrame:down_frame];
    () = msg![env; down_btn setBackgroundColor:clear];
    () = msg![env; down_btn addTarget:delegate
                               action:(env.objc.lookup_selector("deviceModelScrollDown").unwrap())
                     forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; menu_view addSubview:down_btn];

    (button, menu_view, items, thumb_view)
}

fn quick_options_trainer_enabled(options: &Options) -> bool {
    !options.trainer_disabled
}

fn quick_options_trainer_argument(enabled: bool) -> &'static str {
    if enabled {
        "--trainer"
    } else {
        "--no-trainer"
    }
}

fn quick_options_force_composition_argument(enabled: bool) -> &'static str {
    if enabled {
        "--force-composition"
    } else {
        "--no-force-composition"
    }
}

/// Returns `(native_enabled, switch_enabled)` for the probed ANGLE state.
fn quick_options_gles_native_state(options: &Options, angle_available: bool) -> (bool, bool) {
    (options.gles_native || !angle_available, angle_available)
}

/// Launch argument matching the "GLES Native" switch. Always emitted so that
/// the switch's choice overrides the base default and the options files.
fn quick_options_gles_native_argument(enabled: bool) -> &'static str {
    if enabled {
        "--gles-native"
    } else {
        "--no-gles-native"
    }
}

#[cfg(test)]
mod quick_options_trainer_tests {
    use super::*;

    #[test]
    fn trainer_toggle_defaults_off_and_emits_explicit_launch_option() {
        let mut options = Options::default();

        let mut enabled = quick_options_trainer_enabled(&options);
        assert!(!enabled);
        assert_eq!(quick_options_trainer_argument(enabled), "--no-trainer");

        options.parse_argument("--trainer").unwrap();
        enabled = quick_options_trainer_enabled(&options);
        assert!(enabled);
        assert_eq!(quick_options_trainer_argument(enabled), "--trainer");

        options.parse_argument("--no-trainer").unwrap();
        enabled = quick_options_trainer_enabled(&options);
        assert!(!enabled);
        assert_eq!(quick_options_trainer_argument(enabled), "--no-trainer");
    }
}

#[cfg(test)]
mod quick_options_gles_native_tests {
    use super::*;

    #[test]
    fn gles_native_switch_follows_backend_availability_and_option_state() {
        let mut options = Options::default();

        let (mut enabled, mut switch_enabled) = quick_options_gles_native_state(&options, true);
        assert!(!enabled);
        assert!(switch_enabled);
        assert_eq!(
            quick_options_gles_native_argument(enabled),
            "--no-gles-native"
        );

        (enabled, switch_enabled) = quick_options_gles_native_state(&options, false);
        assert!(enabled);
        assert!(!switch_enabled);
        assert_eq!(quick_options_gles_native_argument(enabled), "--gles-native");

        options.parse_argument("--gles-native").unwrap();
        (enabled, switch_enabled) = quick_options_gles_native_state(&options, true);
        assert!(enabled);
        assert!(switch_enabled);
        assert_eq!(quick_options_gles_native_argument(enabled), "--gles-native");

        (enabled, switch_enabled) = quick_options_gles_native_state(&options, false);
        assert!(enabled);
        assert!(!switch_enabled);

        options.parse_argument("--no-gles-native").unwrap();
        (enabled, switch_enabled) = quick_options_gles_native_state(&options, true);
        assert!(!enabled);
        assert!(switch_enabled);
        assert_eq!(
            quick_options_gles_native_argument(enabled),
            "--no-gles-native"
        );
    }
}

#[cfg(test)]
mod quick_options_force_composition_tests {
    use super::*;

    #[test]
    fn switch_argument_explicitly_sets_or_clears_the_option() {
        assert_eq!(
            quick_options_force_composition_argument(true),
            "--force-composition"
        );
        assert_eq!(
            quick_options_force_composition_argument(false),
            "--no-force-composition"
        );
    }
}

#[cfg(test)]
mod app_launch_transition_tests {
    use super::*;

    #[test]
    fn launch_transition_easing_is_smooth_and_clamped() {
        assert_eq!(app_launch_animation_eased_progress(-1.0), 0.0);
        assert_eq!(app_launch_animation_eased_progress(0.0), 0.0);
        assert_eq!(app_launch_animation_eased_progress(0.5), 0.5);
        assert_eq!(app_launch_animation_eased_progress(1.0), 1.0);
        assert_eq!(app_launch_animation_eased_progress(2.0), 1.0);

        let mut previous = 0.0;
        for step in 1..=100 {
            let progress = app_launch_animation_eased_progress(step as f64 / 100.0);
            assert!(progress >= previous);
            previous = progress;
        }
    }
}
