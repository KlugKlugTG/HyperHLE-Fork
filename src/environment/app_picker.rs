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
use crate::frameworks::uikit::ui_view::ui_control::ui_button::UIButtonTypeCustom;
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
use std::sync::atomic::{AtomicI8, Ordering};
use std::time::{Duration, Instant};

/// Pending left (-1) or right (+1) arrow key press, 0 if none. Written by the
/// window event handler and consumed by the picker loop.
static PENDING_ARROW_KEY: AtomicI8 = AtomicI8::new(0);

/// Records an arrow key press for the app picker to act on.
pub fn post_arrow_key(dir: i8) {
    PENDING_ARROW_KEY.store(dir, Ordering::Relaxed);
}

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
    settings_show: bool,
    settings_hide: bool,
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

- (())settingsShow {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).settings_show = true;
}
- (())settingsHide {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).settings_hide = true;
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
        origin: CGPoint { x: 12.0, y: 8.0 },
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
            update_icon_grid(env, &mut icon_grid_stuff, apps, 0, 0);
            Some(icon_grid_stuff)
        }
        Err(e) => {
            let label_frame = CGRect {
                origin: CGPoint { x: 10.0, y: 10.0 },
                size: CGSize {
                    width: app_frame.size.width - 20.0,
                    height: app_picker_grid_bottom(app_frame.size.height) - 20.0,
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

    let mut quick_options_cheat_engine = quick_options_trainer_enabled(&env.options);
    let angle_backend_available = crate::window::angle_backend_available();
    let (mut quick_options_gles_native, quick_options_gles_native_switch_enabled) =
        quick_options_gles_native_state(&env.options, angle_backend_available);
    let quick_options_force_composition_enabled = env.options.force_composition;
    let settings = setup_settings(
        env,
        delegate,
        main_view,
        app_frame,
        quick_options_cheat_engine,
        quick_options_gles_native,
        quick_options_gles_native_switch_enabled,
        quick_options_force_composition_enabled,
        !crate::window::Window::rotatable_fullscreen(),
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

    update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
    update_orientation_buttons(env, &settings.orientation_buttons, quick_options_orientation);
    update_device_model_menu(
        env,
        &settings.device_model_items,
        settings.device_model_thumb,
        quick_options_device_tag,
        quick_options_device_model_scroll,
    );

    () = msg![env; window makeKeyAndVisible];

    let apps_dir = paths::user_data_base_path().join(paths::APPS_DIR);
    let mut current_page: usize = 0;
    // Discard arrow presses made before the picker was shown.
    PENDING_ARROW_KEY.store(0, Ordering::Relaxed);
    // If the user taps the "+" tile, this records the .ipa files that existed
    // at that moment; once a new one shows up, the app list is re-enumerated.
    let mut awaited_ipa: Option<IpaWatch> = None;

    let main_run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
    // If an app is picked, this loop returns. If the user quits touchHLE, the
    // process exits.
    let app_path = loop {
        run_run_loop_single_iteration(env, main_run_loop);
        // Left/right arrow keys switch pages, like tapping the page arrows.
        let arrow = PENDING_ARROW_KEY.swap(0, Ordering::Relaxed);
        if arrow != 0 {
            let settings_view = settings.main_view;
            let settings_open: bool = !msg![env; settings_view isHidden];
            let grid = icon_grid_stuff.as_mut().unwrap();
            let new_page = if arrow > 0 {
                current_page + 1
            } else {
                current_page.wrapping_sub(1)
            };
            if !settings_open && new_page < grid.pages.len() {
                slide_to_page(
                    env,
                    main_run_loop,
                    delegate,
                    grid,
                    apps.as_mut().unwrap(),
                    new_page,
                    arrow as CGFloat,
                );
                current_page = new_page;
            }
        }
        let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(delegate);
        let icon_tapped = std::mem::take(&mut host_obj.icon_tapped);
        if icon_tapped != nil {
            match icon_grid_stuff.as_ref().unwrap().icon_map.get(&icon_tapped) {
                Some(&TappedIcon::App(app_idx)) => {
                    let app_path = apps.as_ref().unwrap()[app_idx].path.clone();
                    echo!("Picked: {}", app_path.display());
                    // iOS-style launch: the icon travels to the middle of the
                    // screen while the screen fades to black.
                    let icon_image: id = msg![env; icon_tapped currentImage];
                    let icon_frame: CGRect = msg![env; icon_tapped frame];
                    play_app_launch_animation(
                        env,
                        main_run_loop,
                        delegate,
                        main_view,
                        app_frame.size,
                        icon_frame,
                        icon_image,
                    );
                    break app_path;
                }
                Some(&TappedIcon::ChangePage(page_idx)) => {
                    let direction: CGFloat = if page_idx > current_page { 1.0 } else { -1.0 };
                    slide_to_page(
                        env,
                        main_run_loop,
                        delegate,
                        icon_grid_stuff.as_mut().unwrap(),
                        apps.as_mut().unwrap(),
                        page_idx,
                        direction,
                    );
                    current_page = page_idx;
                }
                Some(&TappedIcon::AddIpa) => {
                    // Handled by the main loop body below (next iteration).
                    env.objc
                        .borrow_mut::<AppPickerDelegateHostObject>(delegate)
                        .add_ipa = true;
                }
                Some(&TappedIcon::Settings) => {
                    // Handled by the main loop body below (next iteration).
                    env.objc
                        .borrow_mut::<AppPickerDelegateHostObject>(delegate)
                        .settings_show = true;
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
        } else if std::mem::take(&mut host_obj.settings_show) {
            slide_settings_screen(
                env,
                main_run_loop,
                delegate,
                settings.main_view,
                app_frame.size.width,
                true,
            );
        } else if std::mem::take(&mut host_obj.settings_hide) {
            slide_settings_screen(
                env,
                main_run_loop,
                delegate,
                settings.main_view,
                app_frame.size.width,
                false,
            );
        } else if std::mem::take(&mut host_obj.scale_hack_default) {
            quick_options_scale_hack = None;
            update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
        } else if std::mem::take(&mut host_obj.scale_hack1) {
            quick_options_scale_hack = NonZeroU32::new(1);
            update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
        } else if std::mem::take(&mut host_obj.scale_hack2) {
            quick_options_scale_hack = NonZeroU32::new(2);
            update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
        } else if std::mem::take(&mut host_obj.scale_hack3) {
            quick_options_scale_hack = NonZeroU32::new(3);
            update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
        } else if std::mem::take(&mut host_obj.scale_hack4) {
            quick_options_scale_hack = NonZeroU32::new(4);
            update_scale_hack_buttons(env, &settings.scale_hack_buttons, quick_options_scale_hack);
        } else if std::mem::take(&mut host_obj.orientation_default) {
            quick_options_orientation = None;
            update_orientation_buttons(
                env,
                &settings.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_landscape_left) {
            quick_options_orientation = Some(DeviceOrientation::LandscapeLeft);
            update_orientation_buttons(
                env,
                &settings.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_landscape_right) {
            quick_options_orientation = Some(DeviceOrientation::LandscapeRight);
            update_orientation_buttons(
                env,
                &settings.orientation_buttons,
                quick_options_orientation,
            );
        } else if std::mem::take(&mut host_obj.orientation_portrait_upside_down) {
            quick_options_orientation = Some(DeviceOrientation::PortraitUpsideDown);
            update_orientation_buttons(
                env,
                &settings.orientation_buttons,
                quick_options_orientation,
            );
        } else if let Some(tag) = std::mem::take(&mut host_obj.device_model_tag) {
            quick_options_device_tag = Some(tag);
            quick_options_device_model_open = false;
            set_device_model_menu_open(
                env,
                settings.device_model_menu,
                settings.device_model_dimmer,
                false,
            );
            update_device_model_menu(
                env,
                &settings.device_model_items,
                settings.device_model_thumb,
                quick_options_device_tag,
                quick_options_device_model_scroll,
            );
            let title = format!("{} ▼", device_model_label_for_tag(quick_options_device_tag));
            let title_ns = ns_string::from_rust_string(env, title);
            () = msg![env; (settings.device_model_btn)
                setTitle:title_ns forState:UIControlStateNormal];
            release(env, title_ns);
        } else if std::mem::take(&mut host_obj.device_model_toggle) {
            quick_options_device_model_open = !quick_options_device_model_open;
            set_device_model_menu_open(
                env,
                settings.device_model_menu,
                settings.device_model_dimmer,
                quick_options_device_model_open,
            );
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
            () = msg![env; (settings.device_model_btn)
                setTitle:title_ns forState:UIControlStateNormal];
            release(env, title_ns);
        } else if std::mem::take(&mut host_obj.device_model_scroll_up) {
            if quick_options_device_model_scroll > 0 {
                quick_options_device_model_scroll -= 1;
            }
            update_device_model_menu(
                env,
                &settings.device_model_items,
                settings.device_model_thumb,
                quick_options_device_tag,
                quick_options_device_model_scroll,
            );
        } else if std::mem::take(&mut host_obj.device_model_scroll_down) {
            let max_scroll = (settings.device_model_items.len() as isize)
                .saturating_sub(DEVICE_MENU_VISIBLE_ITEMS as isize);
            if quick_options_device_model_scroll < max_scroll {
                quick_options_device_model_scroll += 1;
            }
            update_device_model_menu(
                env,
                &settings.device_model_items,
                settings.device_model_thumb,
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
                        grid.pages = compute_pages(grid.containers[0].cells.len(), new_apps.len());
                        if current_page >= grid.pages.len() {
                            current_page = grid.pages.len() - 1;
                        }
                        let visible = grid.visible;
                        update_icon_grid(env, grid, &mut new_apps, current_page, visible);
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
const APP_PICKER_FOOTER_GAP: CGFloat = 10.0;
const APP_PICKER_GRID_TOP: CGFloat = 44.0;
const APP_PICKER_ICON_ROWS: usize = 4;

const ICON_SIZE: CGSize = CGSize {
    width: 72.0,
    height: 72.0,
};
const ICON_IMAGE_INSET: CGFloat = 9.0;
const ICON_LABEL_TOP_GAP: CGFloat = 2.0;
const ICON_ROW_GAP: CGFloat = 2.0;

fn app_picker_version_label_top(app_height: CGFloat) -> CGFloat {
    app_height - APP_PICKER_VERSION_LABEL_HEIGHT - APP_PICKER_VERSION_LABEL_BOTTOM_INSET
}

/// The bottom edge of the icon grid (the footer area is now empty space).
fn app_picker_grid_bottom(app_height: CGFloat) -> CGFloat {
    app_picker_version_label_top(app_height) - APP_PICKER_FOOTER_GAP
}

fn app_picker_icon_grid_num_rows(app_height: CGFloat, label_height: CGFloat) -> usize {
    let grid_bottom = app_picker_grid_bottom(app_height);
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
    Settings,
}

/// One full set of icon-grid cells on its own container view. Two of these
/// exist so that one page can slide out while the next slides in.
struct PageContainer {
    view: id,
    /// (icon button, name label) pairs.
    cells: Vec<(id, id)>,
}

struct IconGridStuff {
    containers: Vec<PageContainer>,
    /// Index into `containers` of the container currently on screen.
    visible: usize,
    /// Width of the picker screen, used by the page slide animation.
    screen_width: CGFloat,
    placeholder_icon: Option<id>,
    prev_icon: Option<id>,
    next_icon: Option<id>,
    plus_icon: Option<id>,
    settings_icon: Option<id>,
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
        width: 74.0,
        height: 12.0,
    };
    let icon_gap_x: CGFloat = 19.0;
    let icon_gap_y = ICON_LABEL_TOP_GAP + label_size.height + ICON_ROW_GAP;
    let grid_top = APP_PICKER_GRID_TOP;
    let num_rows = app_picker_icon_grid_num_rows(app_frame.size.height, label_size.height);
    let icon_grid_width = (ICON_SIZE.width * num_cols_f) + icon_gap_x * (num_cols_f - 1.0);
    let icon_grid_origin = CGPoint {
        x: (app_frame.size.width - icon_grid_width) / 2.0,
        y: grid_top,
    };

    let icon_tapped_sel = env.objc.lookup_selector("iconTapped:").unwrap();

    let mut containers = Vec::new();
    for container_idx in 0..2 {
        // The container is a full-screen view, so moving it slides all its cells.
        let clear: id = msg_class![env; UIColor clearColor];
        let container = new_view(
            env,
            CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: app_frame.size,
            },
            clear,
        );
        if container_idx != 0 {
            () = msg![env; container setHidden:true];
        }
        () = msg![env; main_view addSubview:container];

        let mut cells = Vec::new();
        for i in 0..(num_cols * num_rows) {
            let col = i % num_cols;
            let row = i / num_cols;

            // Rounding is needed here to avoid a blurry or offset image.
            let icon_frame = CGRect {
                origin: CGPoint {
                    x: (icon_grid_origin.x + (col as CGFloat) * (ICON_SIZE.width + icon_gap_x))
                        .round(),
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
            () = msg![env; container addSubview:icon_button];

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
            () = msg![env; container addSubview:label];

            cells.push((icon_button, label));
        }
        containers.push(PageContainer {
            view: container,
            cells,
        });
    }

    // TODO: Use UIScrollView pagination and UIPageControl once available.
    let total_slots = containers[0].cells.len();
    let pages = compute_pages(total_slots, total_app_count);

    IconGridStuff {
        containers,
        visible: 0,
        screen_width: app_frame.size.width,
        placeholder_icon: None,
        prev_icon: None,
        next_icon: None,
        plus_icon: None,
        settings_icon: None,
        pages,
        icon_map: HashMap::new(),
    }
}

/// Work out which apps go on each page of the icon grid.
///
/// Page 0 reserves its first two slots for the "add IPA" (+) tile and the
/// Settings tile; the remaining slots are used for the prev/next arrows (when
/// relevant) and the apps.
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
        let fixed_slots = usize::from(has_prev) + if page_idx == 0 { 2 } else { 0 };
        let mut app_slots = total_slots.saturating_sub(fixed_slots).max(1);
        let remaining = total_app_count - start;
        if remaining > app_slots {
            // Leave a slot for the "next" arrow.
            app_slots = (app_slots - 1).max(1);
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

/// Fills one page container (`container_idx`) of the icon grid with the apps
/// of `page_idx`. The other container is left alone, so it can keep showing
/// the previous page while a slide animation runs.
fn update_icon_grid(
    env: &mut Environment,
    icon_grid_stuff: &mut IconGridStuff,
    apps: &mut [AppInfo],
    page_idx: usize,
    container_idx: usize,
) {
    let cells = icon_grid_stuff.containers[container_idx].cells.clone();
    for &(icon_button, _) in &cells {
        icon_grid_stuff.icon_map.remove(&icon_button);
    }

    let app_idx_range = icon_grid_stuff.pages[page_idx].clone();
    let have_prev_icon = page_idx != 0;
    let have_next_icon = app_idx_range.end != apps.len();

    let mut icon_iter = cells.iter();

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

        // The Settings app sits right after the "+" tile.
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff
            .settings_icon
            .get_or_insert_with(|| make_settings_app_icon(env));
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, "Settings"))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::Settings);
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

/// Helper to build a `CGRect` from its components.
fn rect(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) -> CGRect {
    CGRect {
        origin: CGPoint { x, y },
        size: CGSize { width, height },
    }
}

fn ui_color(env: &mut Environment, r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) -> id {
    let color: id = msg_class![env; UIColor colorWithRed:r green:g blue:b alpha:a];
    color
}

/// Creates a plain `UIView` with the given frame and background colour.
fn new_view(env: &mut Environment, frame: CGRect, bg: id) -> id {
    let view: id = msg_class![env; UIView alloc];
    let view: id = msg![env; view initWithFrame:frame];
    () = msg![env; view setBackgroundColor:bg];
    view
}

/// Creates a transparent `UILabel` with the given text and style.
fn new_label(
    env: &mut Environment,
    frame: CGRect,
    text: &str,
    font_size: CGFloat,
    bold: bool,
    text_color: id,
) -> id {
    let label: id = msg_class![env; UILabel alloc];
    let label: id = msg![env; label initWithFrame:frame];
    let ns_text = ns_string::from_rust_string(env, text.to_string());
    () = msg![env; label setText:ns_text];
    let font: id = if bold {
        msg_class![env; UIFont boldSystemFontOfSize:font_size]
    } else {
        msg_class![env; UIFont systemFontOfSize:font_size]
    };
    () = msg![env; label setFont:font];
    () = msg![env; label setTextColor:text_color];
    let clear: id = msg_class![env; UIColor clearColor];
    () = msg![env; label setBackgroundColor:clear];
    label
}

fn set_view_x(env: &mut Environment, view: id, x: CGFloat) {
    let mut frame: CGRect = msg![env; view frame];
    frame.origin.x = x;
    () = msg![env; view setFrame:frame];
}

fn lerp_f(a: CGFloat, b: CGFloat, t: CGFloat) -> CGFloat {
    a + (b - a) * t
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Duration of the page slide when pressing the arrow tiles.
const PAGE_SLIDE_DURATION: Duration = Duration::from_millis(320);
/// Duration of the "app icon zooms to the middle, screen fades to black"
/// animation played when an app is picked.
/// Duration of the icon's move to the middle of the screen.
const APP_LAUNCH_MOVE_DURATION: Duration = Duration::from_millis(600);
/// How long the icon rests in the middle before it dissolves.
const APP_LAUNCH_CENTER_HOLD_DURATION: Duration = Duration::from_millis(400);
/// Duration of the dissolve of the icon into the black screen.
const APP_LAUNCH_DISSOLVE_DURATION: Duration = Duration::from_millis(500);
/// How long the black screen stays up before the app takes over.
const APP_LAUNCH_BLACK_HOLD_DURATION: Duration = Duration::from_millis(250);
/// Side length of the app icon at the centre of the launch animation.
const APP_LAUNCH_ICON_SIZE: CGFloat = 120.0;
/// Duration of the settings screen slide (iOS-style push from the right).
const SETTINGS_SLIDE_DURATION: Duration = Duration::from_millis(350);

/// Runs the main run loop for `duration`, calling `step` with the linear
/// progress (0.0 to 1.0) before every iteration. Taps delivered while the
/// animation runs are dropped, because they would land on moving views.
fn animate_for(
    env: &mut Environment,
    run_loop: id,
    delegate: id,
    duration: Duration,
    mut step: impl FnMut(&mut Environment, f64),
) {
    let start = Instant::now();
    loop {
        let t = (start.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
        step(env, t);
        run_run_loop_single_iteration(env, run_loop);
        env.objc
            .borrow_mut::<AppPickerDelegateHostObject>(delegate)
            .icon_tapped = nil;
        if t >= 1.0 {
            break;
        }
    }
}

/// Plays the iOS-style launch animation: the tapped icon travels to the middle
/// of the screen while the screen fades to black. The app then starts on top.
fn play_app_launch_animation(
    env: &mut Environment,
    run_loop: id,
    delegate: id,
    main_view: id,
    screen_size: CGSize,
    icon_frame: CGRect,
    icon: id,
) {
    let black: id = msg_class![env; UIColor blackColor];
    let overlay = new_view(
        env,
        rect(0.0, 0.0, screen_size.width, screen_size.height),
        black,
    );
    () = msg![env; overlay setAlpha:(0.0 as CGFloat)];
    () = msg![env; main_view addSubview:overlay];

    let icon_view: id = msg_class![env; UIImageView alloc];
    let icon_view: id = msg![env; icon_view initWithImage:icon];
    () = msg![env; icon_view setFrame:icon_frame];
    () = msg![env; main_view addSubview:icon_view];

    let target_x = (screen_size.width - APP_LAUNCH_ICON_SIZE) / 2.0;
    let target_y = (screen_size.height - APP_LAUNCH_ICON_SIZE) / 2.0;
    // 1. The icon travels to the middle of the screen and grows.
    animate_for(
        env,
        run_loop,
        delegate,
        APP_LAUNCH_MOVE_DURATION,
        |env, t| {
            let p = ease_in_out_cubic(t.min(1.0)) as CGFloat;
            let frame = rect(
                lerp_f(icon_frame.origin.x, target_x, p),
                lerp_f(icon_frame.origin.y, target_y, p),
                lerp_f(icon_frame.size.width, APP_LAUNCH_ICON_SIZE, p),
                lerp_f(icon_frame.size.height, APP_LAUNCH_ICON_SIZE, p),
            );
            () = msg![env; icon_view setFrame:frame];
            // The black background fades in behind the moving icon.
            () = msg![env; overlay setAlpha:(p as CGFloat)];
        },
    );
    // 2. The icon rests in the middle for a moment.
    animate_for(
        env,
        run_loop,
        delegate,
        APP_LAUNCH_CENTER_HOLD_DURATION,
        |_env, _t| {},
    );
    // 3. The icon dissolves into the black background.
    animate_for(
        env,
        run_loop,
        delegate,
        APP_LAUNCH_DISSOLVE_DURATION,
        |env, t| {
            let p = ease_in_out_cubic(t.min(1.0));
            () = msg![env; icon_view setAlpha:((1.0 - p) as CGFloat)];
        },
    );
    // 4. Hold on the black screen before the app takes over.
    animate_for(
        env,
        run_loop,
        delegate,
        APP_LAUNCH_BLACK_HOLD_DURATION,
        |_env, _t| {},
    );
}

/// Slides the icon grid to `new_page`. `direction` is +1.0 when moving to a
/// later page (the new page comes in from the right) and -1.0 for an earlier
/// one. The two page containers swap roles at the end.
fn slide_to_page(
    env: &mut Environment,
    run_loop: id,
    delegate: id,
    grid: &mut IconGridStuff,
    apps: &mut [AppInfo],
    new_page: usize,
    direction: CGFloat,
) {
    let front = grid.visible;
    let back = front ^ 1;
    let width = grid.screen_width;
    update_icon_grid(env, grid, apps, new_page, back);

    let front_view = grid.containers[front].view;
    let back_view = grid.containers[back].view;
    set_view_x(env, back_view, direction * width);
    () = msg![env; back_view setHidden:false];

    animate_for(env, run_loop, delegate, PAGE_SLIDE_DURATION, |env, t| {
        let p = ease_out_cubic(t) as CGFloat;
        set_view_x(env, front_view, -direction * width * p);
        set_view_x(env, back_view, direction * width * (1.0 - p));
    });

    set_view_x(env, front_view, 0.0);
    () = msg![env; front_view setHidden:true];
    grid.visible = back;
}

/// Slides the settings screen in from the right (`open`) or back out again.
fn slide_settings_screen(
    env: &mut Environment,
    run_loop: id,
    delegate: id,
    settings_view: id,
    width: CGFloat,
    open: bool,
) {
    if open {
        () = msg![env; settings_view setHidden:false];
    }
    animate_for(env, run_loop, delegate, SETTINGS_SLIDE_DURATION, |env, t| {
        let p = ease_out_cubic(t);
        let progress = if open { p } else { 1.0 - p };
        set_view_x(env, settings_view, (width as f64 * (1.0 - progress)) as CGFloat);
    });
    if !open {
        () = msg![env; settings_view setHidden:true];
    }
}

/// Builds the iOS-style "Settings" app icon (grey gear, gloss and rounded
/// corners, like the bundled app icons).
fn make_settings_app_icon(env: &mut Environment) -> id {
    let bytes: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/res/settings_icon.png"
    ));
    let mut image = Image::from_bytes(bytes).unwrap();
    // Same corner radius and gloss as `Bundle::load_icon()`.
    image.round_corners(12.0, /* four_corners: */ true, /* add_sheen: */ true);
    let cg_image = cg_image::from_image(env, image);
    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);
    ui_image
}

/// Blue for the selected segment, light grey for the others (iOS style).
fn update_segment_buttons(env: &mut Environment, buttons: &[id], selected_idx: usize) {
    for (idx, &button) in buttons.iter().enumerate() {
        let selected = idx == selected_idx;
        let bg = if selected {
            ui_color(env, 0.0, 0.478, 1.0, 1.0)
        } else {
            ui_color(env, 0.87, 0.87, 0.89, 1.0)
        };
        let title_color: id = if selected {
            msg_class![env; UIColor whiteColor]
        } else {
            msg_class![env; UIColor blackColor]
        };
        () = msg![env; button setBackgroundColor:bg];
        () = msg![env; button setTitleColor:title_color forState:UIControlStateNormal];
    }
}

fn update_scale_hack_buttons(env: &mut Environment, buttons: &[id], value: Option<NonZeroU32>) {
    update_segment_buttons(env, buttons, value.map_or(0, |v| v.get() as usize));
}

fn update_orientation_buttons(
    env: &mut Environment,
    buttons: &[id],
    value: Option<DeviceOrientation>,
) {
    update_segment_buttons(
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

/// Creates a horizontal row of segment-style buttons inside `parent`.
fn make_segment_buttons(
    env: &mut Environment,
    delegate: id,
    parent: id,
    frame: CGRect,
    buttons: &[(&'static str, &'static str)],
) -> Vec<id> {
    let gap: CGFloat = 4.0;
    let count = buttons.len() as CGFloat;
    let button_width = (frame.size.width - gap * (count - 1.0)) / count;
    let mut result = Vec::new();
    for (i, &(title_text, selector)) in buttons.iter().enumerate() {
        let button_frame = rect(
            frame.origin.x + (i as CGFloat) * (button_width + gap),
            frame.origin.y,
            button_width,
            frame.size.height,
        );
        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
        () = msg![env; button setFrame:button_frame];
        let text = ns_string::get_static_str(env, title_text);
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];
        let label: id = msg![env; button titleLabel];
        let font: id = msg_class![env; UIFont systemFontOfSize:(13.0 as CGFloat)];
        () = msg![env; label setFont:font];
        let layer: id = msg![env; button layer];
        () = msg![env; layer setCornerRadius:(5.0 as CGFloat)];
        let selector = env.objc.lookup_selector(selector).unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; parent addSubview:button];
        result.push(button);
    }
    result
}

/// One row of the settings screen.
#[derive(Clone, Copy)]
enum SettingsRow {
    /// A label with a row of segment buttons below it.
    Segmented(&'static str, &'static [(&'static str, &'static str)]),
    /// The "Device model" row (its dropdown is a root-level overlay).
    DeviceDropdown,
    /// Label and switch: (label, selector, initial state, enabled).
    Toggle(&'static str, &'static str, bool, bool),
}

const SETTINGS_NAV_BAR_HEIGHT: CGFloat = 44.0;
const SETTINGS_TOP_PADDING: CGFloat = 12.0;
const SETTINGS_GROUP_INSET: CGFloat = 16.0;
const SETTINGS_ROW_SIDE_INSET: CGFloat = 16.0;
const SETTINGS_ROW_HEIGHT: CGFloat = 44.0;
const SETTINGS_SEGMENT_ROW_HEIGHT: CGFloat = 58.0;
const SETTINGS_HEADER_HEIGHT: CGFloat = 28.0;
const SETTINGS_SECTION_GAP: CGFloat = 10.0;
/// Width of the device-model dropdown (list plus scrollbar).
const SETTINGS_DEVICE_MENU_WIDTH: CGFloat = 280.0;

fn settings_row_height(row: &SettingsRow) -> CGFloat {
    match row {
        SettingsRow::Segmented(..) => SETTINGS_SEGMENT_ROW_HEIGHT,
        _ => SETTINGS_ROW_HEIGHT,
    }
}

/// The views of the Settings screen that the event loop needs to update.
struct SettingsStuff {
    /// Full-screen settings view (hidden until the Settings tile is tapped).
    main_view: id,
    scale_hack_buttons: [id; 5],
    orientation_buttons: [id; 4],
    /// The button in the "Device model" row. Its title shows the selection.
    device_model_btn: id,
    /// The dropdown list (hidden until the button is tapped).
    device_model_menu: id,
    /// Dims the screen behind the open dropdown; tapping it closes the list.
    device_model_dimmer: id,
    /// One button per choice in `device_model_entries()` order.
    device_model_items: Vec<id>,
    /// The scrollbar thumb shown alongside the dropdown list.
    device_model_thumb: id,
}

/// Views created by [make_device_model_dropdown].
struct DeviceModelDropdown {
    button: id,
    menu: id,
    dimmer: id,
    items: Vec<id>,
    thumb: id,
}

fn set_device_model_menu_open(env: &mut Environment, menu: id, dimmer: id, open: bool) {
    () = msg![env; menu setHidden:(!open)];
    () = msg![env; dimmer setHidden:(!open)];
}

/// Builds the Settings screen: a navigation bar with a "Done" button, and a
/// scrollable list of grouped sections in the style of the iOS Settings app.
fn setup_settings(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    app_frame: CGRect,
    cheat_engine_enabled: bool,
    gles_native_enabled: bool,
    gles_native_switch_enabled: bool,
    force_composition_enabled: bool,
    show_fullscreen_row: bool,
) -> SettingsStuff {
    let width = app_frame.size.width;
    let height = app_frame.size.height;
    // The Settings screen covers the whole window, including the status bar
    // strip above the application frame. Nothing is clipped by the emulator,
    // so scrolled content must not be able to show up in that strip.
    let status_offset = app_frame.origin.y;
    let root_size = CGSize {
        width,
        height: height + status_offset,
    };

    let grouped_bg = ui_color(env, 0.937, 0.937, 0.957, 1.0);
    let settings_view = new_view(
        env,
        rect(0.0, -status_offset, width, height + status_offset),
        grouped_bg,
    );
    () = msg![env; settings_view setHidden:true];
    () = msg![env; super_view addSubview:settings_view];

    let clear: id = msg_class![env; UIColor clearColor];
    let black: id = msg_class![env; UIColor blackColor];
    let white: id = msg_class![env; UIColor whiteColor];
    let gray_text = ui_color(env, 0.43, 0.43, 0.45, 1.0);
    let blue = ui_color(env, 0.0, 0.478, 1.0, 1.0);

    // Navigation bar with the title and a "Done" button.
    let nav_bar_bg = ui_color(env, 0.97, 0.97, 0.98, 1.0);
    let nav_bar = new_view(
        env,
        rect(0.0, 0.0, width, SETTINGS_NAV_BAR_HEIGHT + status_offset),
        nav_bar_bg,
    );
    () = msg![env; settings_view addSubview:nav_bar];
    let nav_separator_color = ui_color(env, 0.78, 0.78, 0.8, 1.0);
    let nav_separator = new_view(
        env,
        rect(0.0, SETTINGS_NAV_BAR_HEIGHT + status_offset - 1.0, width, 1.0),
        nav_separator_color,
    );
    () = msg![env; settings_view addSubview:nav_separator];

    let title_color = ui_color(env, 0.1, 0.1, 0.1, 1.0);
    let title = new_label(
        env,
        rect(60.0, status_offset, width - 120.0, SETTINGS_NAV_BAR_HEIGHT),
        "Settings",
        17.0,
        true,
        title_color,
    );
    () = msg![env; title setTextAlignment:UITextAlignmentCenter];
    () = msg![env; settings_view addSubview:title];

    let done: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    () = msg![env; done setFrame:(rect(width - 72.0, status_offset, 64.0, SETTINGS_NAV_BAR_HEIGHT))];
    let done_text = ns_string::get_static_str(env, "Done");
    () = msg![env; done setTitle:done_text forState:UIControlStateNormal];
    // FIXME: manually calling layoutSubviews shouldn't be needed?
    () = msg![env; done layoutSubviews];
    () = msg![env; done setTitleColor:blue forState:UIControlStateNormal];
    let done_label: id = msg![env; done titleLabel];
    let done_font: id = msg_class![env; UIFont boldSystemFontOfSize:(17.0 as CGFloat)];
    () = msg![env; done_label setFont:done_font];
    let hide_selector = env.objc.lookup_selector("settingsHide").unwrap();
    () = msg![env; done addTarget:delegate
                           action:hide_selector
                 forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; settings_view addSubview:done];

    // Scrollable content below the navigation bar.
    let scroll: id = msg_class![env; UIScrollView alloc];
    let scroll: id = msg![env; scroll initWithFrame:(rect(
        0.0,
        SETTINGS_NAV_BAR_HEIGHT + status_offset,
        width,
        height - SETTINGS_NAV_BAR_HEIGHT,
    ))];
    () = msg![env; scroll setBackgroundColor:clear];
    // Clip the scrolled content to the scroll view, so it never slides up
    // over the navigation bar.
    () = msg![env; scroll setClipsToBounds:true];
    () = msg![env; settings_view addSubview:scroll];
    // The navigation bar must stay on top of the scrolled content.
    () = msg![env; settings_view bringSubviewToFront:nav_bar];
    () = msg![env; settings_view bringSubviewToFront:nav_separator];
    () = msg![env; settings_view bringSubviewToFront:title];
    () = msg![env; settings_view bringSubviewToFront:done];

    let mut sections: Vec<(&'static str, Vec<SettingsRow>)> = Vec::new();
    let mut display_rows = vec![
        SettingsRow::Segmented(
            "Scale hack",
            &[
                ("Default", "scaleHackDefault"),
                ("Off", "scaleHack1"),
                ("2×", "scaleHack2"),
                ("3×", "scaleHack3"),
                ("4×", "scaleHack4"),
            ],
        ),
        SettingsRow::Segmented(
            "Orientation",
            &[
                ("Default", "orientationDefault"),
                ("←", "orientationLandscapeLeft"),
                ("→", "orientationLandscapeRight"),
                ("↓", "orientationPortraitUpsideDown"),
            ],
        ),
    ];
    if show_fullscreen_row {
        display_rows.push(SettingsRow::Toggle(
            "Fullscreen (override)",
            "fullscreen:",
            false,
            true,
        ));
    }
    sections.push(("DISPLAY", display_rows));
    sections.push((
        "GRAPHICS",
        vec![
            SettingsRow::Toggle(
                "GLES Native",
                "glesNative:",
                gles_native_enabled,
                gles_native_switch_enabled,
            ),
            SettingsRow::Toggle(
                "May fix graphics issues.",
                "forceComposition:",
                force_composition_enabled,
                true,
            ),
            SettingsRow::Toggle("Show FPS", "showFPS:", false, true),
            SettingsRow::Toggle("Trace GL errors", "traceGLErrors:", false, true),
        ],
    ));
    sections.push(("DEVICE", vec![SettingsRow::DeviceDropdown]));
    sections.push((
        "EMULATION",
        vec![
            SettingsRow::Toggle("Cheat Engine", "cheatEngine:", cheat_engine_enabled, true),
            SettingsRow::Toggle("Network access", "network:", false, true),
            SettingsRow::Toggle(
                "Use analog sticks for tilt controls",
                "analogStickTiltControls:",
                true,
                true,
            ),
        ],
    ));

    let inner_width = width - 2.0 * SETTINGS_GROUP_INSET;
    let side = SETTINGS_ROW_SIDE_INSET;
    let mut scale_hack_buttons: Vec<id> = Vec::new();
    let mut orientation_buttons: Vec<id> = Vec::new();
    let mut dropdown: Option<DeviceModelDropdown> = None;

    let mut y: CGFloat = SETTINGS_TOP_PADDING;
    for (header, rows) in sections {
        let header_label = new_label(
            env,
            rect(SETTINGS_GROUP_INSET + 12.0, y, inner_width - 12.0, 18.0),
            header,
            13.0,
            false,
            gray_text,
        );
        () = msg![env; scroll addSubview:header_label];
        y += SETTINGS_HEADER_HEIGHT;

        let group_height: CGFloat = rows.iter().map(settings_row_height).sum();
        let group = new_view(
            env,
            rect(SETTINGS_GROUP_INSET, y, inner_width, group_height),
            white,
        );
        let group_layer: id = msg![env; group layer];
        () = msg![env; group_layer setCornerRadius:(10.0 as CGFloat)];
        () = msg![env; scroll addSubview:group];

        let row_count = rows.len();
        let mut row_y: CGFloat = 0.0;
        for (index, row) in rows.iter().enumerate() {
            match *row {
                SettingsRow::Segmented(label, buttons) => {
                    let label_view = new_label(
                        env,
                        rect(side, row_y + 6.0, inner_width - 2.0 * side, 18.0),
                        label,
                        15.0,
                        false,
                        black,
                    );
                    () = msg![env; group addSubview:label_view];
                    let seg_buttons = make_segment_buttons(
                        env,
                        delegate,
                        group,
                        rect(side, row_y + 28.0, inner_width - 2.0 * side, 24.0),
                        buttons,
                    );
                    if buttons[0].1 == "scaleHackDefault" {
                        scale_hack_buttons = seg_buttons;
                    } else {
                        orientation_buttons = seg_buttons;
                    }
                }
                SettingsRow::DeviceDropdown => {
                    let label_view = new_label(
                        env,
                        rect(side, row_y + 11.0, 150.0, 22.0),
                        "Device model",
                        16.0,
                        false,
                        black,
                    );
                    () = msg![env; group addSubview:label_view];
                    let menu_origin = CGPoint {
                        x: (width - SETTINGS_DEVICE_MENU_WIDTH) / 2.0,
                        y: SETTINGS_NAV_BAR_HEIGHT + status_offset + 12.0,
                    };
                    dropdown = Some(make_device_model_dropdown(
                        env,
                        delegate,
                        group,
                        settings_view,
                        rect(inner_width - side - 170.0, row_y + 7.0, 170.0, 30.0),
                        menu_origin,
                        root_size,
                    ));
                }
                SettingsRow::Toggle(label_text, selector, default_state, enabled) => {
                    let switch_width: CGFloat = 94.0;
                    let label_view = new_label(
                        env,
                        rect(
                            side,
                            row_y + 4.0,
                            inner_width - 2.0 * side - switch_width - 8.0,
                            36.0,
                        ),
                        label_text,
                        16.0,
                        false,
                        black,
                    );
                    () = msg![env; label_view setTextAlignment:UITextAlignmentLeft];
                    // Long labels wrap onto a second line, as in iOS Settings.
                    () = msg![env; label_view setNumberOfLines:2];
                    () = msg![env; group addSubview:label_view];

                    let switch: id = msg_class![env; UISwitch alloc];
                    let switch: id = msg![env; switch initWithFrame:(rect(
                        inner_width - side - switch_width,
                        row_y + 8.5,
                        switch_width,
                        27.0,
                    ))];
                    () = msg![env; switch setOn:default_state];
                    () = msg![env; switch setEnabled:enabled];
                    let selector = env.objc.lookup_selector(selector).unwrap();
                    () = msg![env; switch addTarget:delegate
                                             action:selector
                                   forControlEvents:UIControlEventValueChanged];
                    () = msg![env; group addSubview:switch];
                }
            }
            row_y += settings_row_height(row);
            // Hairline separator between rows, inset from the left like iOS.
            if index + 1 < row_count {
                let separator_color = ui_color(env, 0.85, 0.85, 0.87, 1.0);
                let separator = new_view(
                    env,
                    rect(side, row_y - 1.0, inner_width - side, 1.0),
                    separator_color,
                );
                () = msg![env; group addSubview:separator];
            }
        }
        y += group_height + SETTINGS_SECTION_GAP;
    }

    // Footer with the build identifier.
    let footer = new_label(
        env,
        rect(0.0, y, width, 18.0),
        &format!("{HYPERHLE_FORK_NAME} ({})", crate::COMMIT_HASH),
        12.0,
        false,
        gray_text,
    );
    () = msg![env; footer setTextAlignment:UITextAlignmentCenter];
    () = msg![env; scroll addSubview:footer];
    y += 30.0;

    () = msg![env; scroll setContentSize:(CGSize { width, height: y + 24.0 })];

    let dropdown = dropdown.expect("the settings screen always has a device model row");
    SettingsStuff {
        main_view: settings_view,
        scale_hack_buttons: scale_hack_buttons.try_into().unwrap(),
        orientation_buttons: orientation_buttons.try_into().unwrap(),
        device_model_btn: dropdown.button,
        device_model_menu: dropdown.menu,
        device_model_dimmer: dropdown.dimmer,
        device_model_items: dropdown.items,
        device_model_thumb: dropdown.thumb,
    }
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

/// Build the "Device model" dropdown: a toggle button in the settings row,
/// plus a (initially hidden) list that is shown as an overlay on the whole
/// Settings screen, with a dimmer behind it. The list contains a
/// vertically-scrollable list of every choice from [device_model_entries], a
/// scrollbar track + thumb, and transparent up/down scroll arrows. Each list
/// item is wired to the delegate's `deviceModel:` selector and tagged with its
/// choice; the arrows fire `deviceModelScrollUp` / `deviceModelScrollDown`.
fn make_device_model_dropdown(
    env: &mut Environment,
    delegate: id,
    group_view: id,
    root_view: id,
    button_frame: CGRect,
    menu_origin: CGPoint,
    root_size: CGSize,
) -> DeviceModelDropdown {
    let list_width: CGFloat = 256.0;
    let scrollbar_width: CGFloat = 24.0;
    let dark_gray: id = msg_class![env; UIColor darkGrayColor];

    // Dimmer covering the whole Settings screen while the list is open.
    let dimmer: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    () = msg![env; dimmer setFrame:(rect(0.0, 0.0, root_size.width, root_size.height))];
    let dim_color = ui_color(env, 0.0, 0.0, 0.0, 0.35);
    () = msg![env; dimmer setBackgroundColor:dim_color];
    () = msg![env; dimmer addTarget:delegate
                             action:(env.objc.lookup_selector("deviceModelToggle").unwrap())
                   forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; dimmer setHidden:true];
    () = msg![env; root_view addSubview:dimmer];

    // The toggle button, shown in the "Device model" row.
    let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    let initial_title = format!("{} ▼", device_model_label_for_tag(None));
    let text = ns_string::from_rust_string(env, initial_title);
    () = msg![env; button setTitle:text forState:UIControlStateNormal];
    release(env, text);
    let blue = ui_color(env, 0.0, 0.478, 1.0, 1.0);
    () = msg![env; button setTitleColor:blue forState:UIControlStateNormal];
    () = msg![env; button setFrame:button_frame];
    () = msg![env; button layoutSubviews];
    let button_label: id = msg![env; button titleLabel];
    let button_font: id = msg_class![env; UIFont systemFontOfSize:(16.0 as CGFloat)];
    () = msg![env; button_label setFont:button_font];
    () = msg![env; button addTarget:delegate
                             action:(env.objc.lookup_selector("deviceModelToggle").unwrap())
                   forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; group_view addSubview:button];

    // The dropdown list, placed on the Settings screen (not inside the
    // scroll view, so it is never clipped). Clipped to its own bounds.
    let visible_menu_height = (DEVICE_MENU_VISIBLE_ITEMS as CGFloat) * DEVICE_MENU_ITEM_HEIGHT;
    let menu_frame = CGRect {
        origin: menu_origin,
        size: CGSize {
            width: SETTINGS_DEVICE_MENU_WIDTH,
            height: visible_menu_height,
        },
    };
    let menu_view: id = msg_class![env; UIView alloc];
    let menu_view: id = msg![env; menu_view initWithFrame:menu_frame];
    () = msg![env; menu_view setBackgroundColor:dark_gray];
    () = msg![env; menu_view setClipsToBounds:true];
    () = msg![env; menu_view setHidden:true];
    () = msg![env; root_view addSubview:menu_view];

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

    DeviceModelDropdown {
        button,
        menu: menu_view,
        dimmer,
        items,
        thumb: thumb_view,
    }
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
