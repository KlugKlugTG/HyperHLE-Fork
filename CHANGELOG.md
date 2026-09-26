# Changelog

This will list notable changes from release to release, and credit the people who contributed them. This mainly covers changes that are visible to end users, so please look at the commit history if you want to know all the details.

Names preceded by an @ are GitHub usernames.

Lists of new working apps are a guideline, not a guarantee of support, and are not comprehensive. Credits for new working apps indicate someone who put a lot of effort into getting that particular app working, but compatibility is always a cumulative collaborative effort. The “Various small contributions” in the changelog often add up in a big way.

Changes are categorised as follows:

* Compatibility: changes that affect which apps work in touchHLE.
* Quality and performance: changes that don't affect which apps work, but do affect the quality of the experience.
* Usability: changes to features of the emulator unrelated to the above, e.g. new input methods.
* Other: when none of the above seem to fit.

## NEXT

Compatibility:

- `realpath()` no longer aborts the guest when the app passes `NULL` as the resolved-path buffer (a valid POSIX usage that mallocs the result); it now allocates the buffer instead. It also sets `errno`/returns `NULL` on unreadable paths instead of failing the whole call. `dirname(3)` is now implemented, along with `getpwuid_r(3)` (a single stub `root`/`mobile` user with the app-container home directory) and `sysconf(_SC_GETPW_R_SIZE_MAX)`. Together these fix Unity's startup path (`rvmStartup` → `getenv("HOME")` fallback chain) for Unity games such as Deep Town. (@KlugKlugTG)

- New working apps:
  - Devil May Cry 4 Refrain (@hikari-no-yume)
  - Amerzone Pt1 (@ciciplusplus)
  - Eternal Legacy (@ciciplusplus)
  - Dungeon Hunter 2 (@ciciplusplus)
  - N.O.V.A. 2: The Hero Rises Again (@ciciplusplus)
  - Star Battalion (@ciciplusplus)
  - Ice Age: Dawn of the Dinosaurs (@ciciplusplus)
  - Zombieville (@ciciplusplus)
  - Doom Resurrection (@ciciplusplus)
  - Ace Combat Xi (@alborrajo)
  - Fruit Ninja (@acieslewicz, @ciciplusplus)
  - Asphalt 6 (@ciciplusplus)
  - World of Goo (@ciciplusplus)
- API support improvements:
  - `-[NSObject performSelectorOnMainThread:withObject:waitUntilDone:]` now queues `waitUntilDone:NO` calls made on the main thread for the next run-loop pass instead of invoking them inline. This prevents asynchronous startup callbacks from observing partially initialized state and fixes Battleship FREE's age/terms flow stalling before its first rendered frame.
  - `NSBundle` now retains cached bundle instances, and `UINib` honors the bundle argument instead of asserting that every nib comes from the main app bundle. This allows Battleship FREE to load its nested age-verification nib and localized strings.
  - Bundled Mach-O dependencies are loaded transitively, and the guest ARM SJLJ unwinder is preferred when present instead of being replaced by host stubs; this lets C++ exceptions unwind normally during game startup.
  - `CFURLCreateStringByReplacingPercentEscapesUsingEncoding()` is now implemented per Apple's documentation instead of returning the input string unchanged: every `%XX` escape is decoded using the specified encoding, characters named in `charactersToLeaveEscaped` keep their escape sequences, invalid or incomplete escape sequences return `NULL`, and passing an empty string removes all escapes. Previously apps that used it to unescape URLs got escaped strings back.
  - `AudioFileGetGlobalInfoSize()` and `AudioFileGetGlobalInfo()` now answer the documented global info properties `kAudioFileGlobalInfo_ReadableTypes`, `kAudioFileGlobalInfo_WritableTypes` and `kAudioFileGlobalInfo_AvailableFormatIDs` (with the documented file-type specifier) instead of returning `kAudioFileUnsupportedPropertyError`, and follow the documented buffer-size protocol (`kAudioFileBadPropertySizeError` for undersized buffers) otherwise.
  - `-[UIViewController dismissMoviePlayerViewControllerAnimated]` now dismisses the presented movie player view controller with the standard transition (previously a no-op stub), so apps that show a full-screen movie on launch can be closed again.
  - `NSBlockOperation` is now implemented per Apple's documentation: `+blockOperationWithBlock:`, `-addExecutionBlock:`, `-executionBlocks` and `-main` (blocks run in the order they were added), and `-[NSOperationQueue addOperationWithBlock:]` uses it instead of logging a warning and doing nothing. Apps that queue work as blocks now execute it.
  - `-[UIScreen brightness]` / `-setBrightness:` and `-wantsSoftwareDimming` / `-setWantsSoftwareDimming:` are now implemented per Apple's documentation: the setter clamps brightness to the documented 0.0–1.0 range and both properties are stored and returned by their getters (previously stubs that logged and discarded the value).
  - `AudioFileOptimize()` now validates the audio file handle and returns `kAudioFileSuccess` for open files (`kAudioFileNotOpenError` otherwise), matching the documented behavior that optimization is a hint which never fails for a valid file (previously always returned `kAudioFileOperationNotSupportedError`).
  - `-[NSProcessInfo operatingSystemVersionString]` now reports the same version as `-operatingSystemVersion` (previously a second, stale declaration returned a fixed iOS 3.1.3 string, which also made the duplicate-selector self-test fail).
  - Removed duplicate `UIView` `layoutIfNeeded`/`setNeedsLayout` declarations so the dylib export self-tests pass again; the real implementations (which call `layoutSubviews`) are used.
  - Various small contributions. (@hikari-no-yume, @ciciplusplus, @zazatree, @abnormalmaps, @alborrajo, @acieslewicz)
  - Implemented the Objective-C runtime functions `property_getName()` and `property_getAttributes()` (previously return-0 stubs), and the `-[NSObject dictionaryWithValuesForKeys:]` Key-Value Coding method. This fixes apps whose embedded SDKs use runtime property introspection to serialize objects (e.g. Spy Mouse HD's Burstly ad SDK, which was stuck in a network/loading loop).
  - `class_getProperty()` no longer special-cases `[UIScreen scale]` to return `NULL`. Now that declared `@property` metadata is parsed from the app binary, the function walks the class hierarchy and returns the real `objc_property_t`, matching Apple's documented behaviour. The old hard-coded `NULL` could make apps that probe for the `scale` property mis-detect the device's screen capabilities.
  - `-[UIViewController presentModalViewController:animated:]` now falls back to the first visible `UIWindow` when neither the presenting controller's view nor the application's key window can supply one. Previously a modal presentation by a controller that wasn't yet attached to a window (e.g. the full-screen movie player some games show on launch) silently did nothing.
  - Removed duplicate Mach `thread_suspend()`/`thread_resume()` and `wcstoul()` symbol exports. Two dylib export tables each registered these functions, so the dynamic linker bound the first (less robust) copy and the `no_duplicate_functions` self-test panicked; the canonical bounds-checked implementations in `mach::thread_info` and `libc::wchar` are now the only ones exported.
  - C++ Itanium ABI type_info objects and `__cxxabiv1` type_info vtables are now resolved when they are referenced through the `__nl_symbol_ptr` table, not only through external relocations. Previously a symbol like `__ZTIPKc` (typeinfo for `char const*`), which the bundled libstdc++/libc++ of some iOS 8 apps emits into the non-lazy pointer table, was left NULL and caused a NULL-page crash the first time the app's C++ code touched RTTI. This fixes apps such as [Team Umizoomi Math Racer](https://github.com/HyperHLE/HyperHLE/issues/204).
  - Several changes have been made to fix certain apps and games that should appear in landscape, but previously were displayed stretched, cropped and/or un-rotated:
    - If an app requires a landscape orientation in the `UIInterfaceOrientation` or `UISupportedInterfaceOrientations` keys of its `Info.plist`, touchHLE will now rotate the virtual device at startup. (@hikari-no-yume)
    - If an app overrides the `shouldAutorotateToInterfaceOrientation:` method in a `UIViewController`, and the virtual device is in a landscape orientation, touchHLE will now apply a rotation transform to the root view when it is added to a window. (@hikari-no-yume)
    - Fixed a very old assumption that the backing store of a `CAEAGLLayer` should always be 320×480 pixels. (@hikari-no-yume)
  - Support for iPad device family. Device family is deduced from the app bundle, but user can also override it with `--device-family=` option. (@ciciplusplus)
  - Implicit Core Animation animations: changing an animatable `CALayer` property (`bounds`, `position`, `anchorPoint`, `opacity`, `hidden`, `backgroundColor`, `cornerRadius`) outside an explicit transaction now creates a default `CABasicAnimation` via the current `CATransaction`, while `UIView` backing layers keep implicit animations disabled so existing `UIView` animation handling is unaffected. (ported from touchHLE)
  - Objective-C `+load` methods are now sent during class initialization, before any `+initialize`, matching the runtime's ordering guarantee. (ported from touchHLE)
  - `NSGarbageCollector` (with `+defaultCollector` returning `nil`, as on iOS), `NSBundle` localized `.strings` loading in the standard (non-property-list) format, and `+[NSObject willChangeValueForKey:]`/`didChangeValueForKey:` integration. (ported from touchHLE)
  - `dictionaryWithObjectsAndKeys:`/`initWithObjectsAndKeys:` now match Apple's documented vararg contract: the list is `(object1, key1, ...)`, terminated by a `nil` *object*, so a `nil` first object returns an empty dictionary without reading further varargs, and a `nil` *key* now skips just that pair and keeps parsing the rest (previously the whole dictionary was abandoned, losing every valid pair after it). Combined with the existing `-[NSMutableDictionary setObject:forKey:]` nil-key guard, apps whose ad/SDK layers build metadata conditionally (e.g. skipping absent keys) no longer lose data or abort on startup.
  - The bundled dynamic libraries, libgcc and libstdc++, have been updated to their iOS 4.0.1 versions. (@ciciplusplus)
  - Support for NIBArchive NIB file format decoding. (@ciciplusplus)
  - `raise()` and the signal dispositions installed with `signal()`/`sigaction()` are now honoured. `raise()` no longer hits the dynamic linker's return-0 stub: when a handler is installed it is called synchronously on the calling guest thread, `SIG_IGN` is ignored as documented, and a fatal default action ends the guest session through the same controlled recovery path as `abort()` (which now raises `SIGABRT` first, so crash reporters and Unity's unhandled-exception shim run). `pthread_kill()` delivers to the calling thread through the same machinery (previously a silent return-0 stub) and reports `ESRCH` for unknown thread handles, and the `sys_siglist` signal-name table is exported.
  - New `WatchConnectivity.framework` implementation: `WCSession` (with `+isSupported`, `+defaultSession`, delegate/activation-state handling, `sendMessage:replyHandler:errorHandler:` and the transfer APIs), `WCSessionUserInfoTransfer`, `WCSessionFile` and `WCSessionFileTransfer` exist as real classes, and `WCErrorDomain` is exported. Since there is no paired Apple Watch, the session behaves like one on an iPhone without a watch: activation succeeds, `isPaired`/`isWatchAppInstalled`/`isReachable` are `NO`, and send/transfer APIs report the documented `WCErrorDomain` errors instead of silently pretending to succeed. Previously every app that probed `[WCSession isSupported]` was told the class was unimplemented.
  - `-[NSBundle localizedInfoDictionary]` now returns the bundle's localized information-property-list, i.e. the `InfoPlist.strings` of the preferred localization (`en.lproj`, `English.lproj` or `Base.lproj` fallbacks as with other resources) layered over the plain `Info.plist` contents, instead of returning `infoDictionary` unchanged. Previously `CFBundleGetLocalInfoDictionary()` and apps that read keys such as `CFBundleDisplayName` from this dictionary saw untranslated values.
- Switch to coroutine based threading system. This solved [some compatibility issues](https://github.com/touchHLE/touchHLE/issues/119) and improved performance in some games. (@abnormalmaps)

Quality and performance:

- Fullscreen `CAEAGLLayer` detection now evaluates nested layer geometry in screen space and accepts a single exact UIKit device-orientation rotation when the transformed layer still covers the screen. Landscape games that use UIKit autorotation were incorrectly sent through `glReadPixels()` plus software Core Animation composition even on GPU-present configurations; they can now use the existing GPU presenter. The existing native ES 1.1 `auto` readback policy and explicit readback mode are unchanged, and arbitrary transforms/non-fullscreen layers still use the safe compositor path.
- Major GLES presentation and draw-call overhead optimisation pass, plus stability fixes for everything that puts the picture on screen:
  - Frames rendered into a fullscreen `CAEAGLLayer` are now presented on the GPU on every backend (the renderbuffer is copied into a texture and drawn into the window). Previously, on every native OpenGL ES 1.1 backend — i.e. for every ES 1.1 game on Android, including the bundled ANGLE driver — each frame was instead pulled back to system RAM with `glReadPixels()` (a full GPU pipeline stall), re-uploaded as a texture and pushed through the Core Animation compositor. That round trip was by far the largest per-frame cost on Android. The readback route is still available as `--present-mode=readback` (or `TOUCHHLE_PRESENT_MODE=readback`) for broken vendor drivers, and the default `--present-mode=auto` verifies the GPU route during the first seconds of a session by sampling a few pixels of the source and of the window: if the window stays black while the app's frame has content, it switches to readback automatically and says so in the log.
  - The presenters no longer issue a `glFinish()` on every frame before copying the renderbuffer; the copy is ordered after the app's draws by the driver anyway, and the forced pipeline drain prevented the GPU from working on one frame while the emulator prepared the next. `--present-finish` (or `TOUCHHLE_PRESENT_FINISH=1`) brings the old behaviour back for drivers that need it.
  - The presented window always has an opaque alpha channel now (the ES 2.0 present shader writes alpha 1.0, the ES 1.1 path masks alpha writes), so an app that leaves alpha < 1 in its renderbuffer can no longer come out darkened or see-through on window surfaces whose alpha the OS compositor honours (Android).
  - `gl*Pointer`, `glVertexAttribPointer`, `glDrawArrays` and `glDrawElements` no longer ask the host driver about the buffer bindings and the fog state on every call (each such query came with a `glGetError()` round-trip to swallow the errors strict drivers raise for it — up to nine extra driver calls per draw). That state is now mirrored per context as the guest sets it, and generic-vertex-attribute guards are skipped entirely for the fixed-function apps that never enable such attributes.
  - The ES 1.1 present path probes which capability enums the driver rejects once per context instead of wrapping every capability save/restore of every frame in error-drain loops (~100 fewer GL calls per presented frame), and the ES 2.0 presenter only clears the colour buffer of the (depth/stencil-less) window.
  - The compositor updates layer textures in place with `glTexSubImage2D` when their size hasn't changed (instead of reallocating them with `glTexImage2D`), no longer clones a `CAEAGLLayer`'s full frame of pixels when building its presentation layer, and the readback presenter recycles its pixel buffer instead of allocating a fresh one every frame.
  - Stability: `renderbufferStorage:fromDrawable:` clamps absurd renderbuffer sizes instead of panicking on the `GLsizei` conversion, error-queue drains in the presenter are bounded so a sticky driver error can't hang the emulator, and the per-thread current-context lookup on every GL call is a single hash lookup instead of two.
- Android frame-rate pass for OpenGL ES 2.0 games (e.g. N.O.V.A. 3, Asphalt), which typically ran at half the display refresh rate on this fork while the same games reach the full rate on forks that use the vendor's GL driver without vsync:
  - Apps whose executable only imports OpenGL ES 2.0 shader entry points use the vendor's native driver instead of bundled ANGLE. For ES 1.1-capable apps and the app picker, `auto` selects ANGLE only when Android exposes an Adreno KGSL GPU; other or unrecognized GPUs stay on the system driver rather than assuming ANGLE's Vulkan backend is supported. `--gl-driver=angle|native` (or `TOUCHHLE_GL_DRIVER`) overrides the automatic choice.
  - Vsync is now off by default on Android (`--vsync=auto`). HyperHLE paces frames itself and the Android compositor is synchronised to the display anyway, so a blocking `eglSwapBuffers` could not prevent tearing — it could only stall the emulator thread, and a frame that took slightly longer than one refresh then cost two (60 FPS → 30 FPS). `--vsync=on|off` (or `TOUCHHLE_VSYNC`) sets it explicitly on every platform; desktop defaults are unchanged.
  - The emulator thread now runs at Android's "display" scheduling priority and reports its per-frame CPU time to the system performance hint manager (ADPF, Android 13+), so the CPU governor keeps the core clocked for the emulated workload rather than for the idle time between frames — the usual reason an emulated game that could run at 60 FPS settles at 30 after a few seconds. `--no-perf-hints` (or `TOUCHHLE_PERF_HINTS=0`) turns both off.
- `CMDeviceMotion`/`CMAttitude` now expose the filter's continuous, full-3D quaternion (with pitch/roll/yaw decomposed from it) instead of a quaternion rebuilt from two gravity angles with a forced zero yaw. The two-angle construction had seams exactly in the poses people game in; near a seam a tiny physical twist produced an enormous frame-to-frame attitude delta and camera code consuming the attitude spun the view to maximum. `attitude`/`quaternion`/`rotationMatrix` are now always mutually consistent. (@KlugKlugTG)
- The device-motion attitude filter now also learns the gyroscope's constant bias while the device is provably stationary (judging by accelerometer stability rather than by the gyro reading itself, so even a large constant offset converges), and its gravity input is low-passed first — together this removes both the bias-driven attitude wander and the resting-hand jitter seen in tilt-camera games. Diagnostic: setting `TOUCHHLE_NO_MOTION_GYRO` disables gyro integration entirely (for hosts with a genuinely unusable gyroscope signal).
- `CMAttitude` now implements `rotationMatrix` and `multiplyByInverseOfAttitude:` — the standard way games calibrate a neutral pose for tilt-driven 3D cameras (store a reference attitude, then read camera-relative attitude). Previously these were unimplemented, so games fell back to the absolute world-frame attitude — which legitimately sits near its extremes in the upright gaming hold — and a small tilt could swing the camera through its whole range. (@KlugKlugTG)
- `CMDeviceMotion` is now produced by a small gyroscope+accelerometer sensor-fusion filter (attitude tracked by integrating the real gyroscope and continuously corrected towards gravity), instead of deriving roll/pitch straight from the raw gravity vector. The old approach blew up in the typical upright, landscape gaming hold — where gravity's out-of-screen component is ~0 — so a small tilt could swing the estimated attitude straight to its extremes (games like Asphalt 7 would snap the camera to maximum on the slightest tilt). `userAcceleration` is now also reported (raw acceleration minus the fused gravity estimate). For smoothness the visible motion is dominated by the continuous gyroscope integration, with gravity only gently correcting long-term drift, and the gravity correction is automatically distrusted whenever the accelerometer magnitude deviates from 1g (device being shaken), so sensor noise and hand jitter no longer leak into the on-screen camera. (@KlugKlugTG)
- Fixed `CMDeviceMotion.attitude` reporting pitch and roll about the wrong device axes (they were effectively swapped, with mismatched signs, and inconsistent with the returned quaternion). Games that tilt-steer or swing a 3D camera using Core Motion device motion (e.g. Asphalt 7's tilt camera) should now move the view in the correct direction for the actual device rotation. (@KlugKlugTG)
- Added a per-game accelerometer escape hatch for games whose tilt controls come out mirrored or sideways on the host device: set the environment variable `TOUCHHLE_ACCELEROMETER_AXES` to a comma-separated list choosing any of `swap`, `flipx`, `flipy` (e.g. `TOUCHHLE_ACCELEROMETER_AXES=swap,flipy`). It applies to real hardware-sensor data only and is read once at startup.
- The Objective-C runtime's hot lookup tables (object map, method tables, initialized-classes set and related bookkeeping) now use a fast integer-oriented hasher instead of the standard-library default SipHash, cutting a handful of hashes done for every guest message send and every retain/release/autorelease down to a multiply-rotate each.
- Two compatibility probes that ran on every single guest message send (NSArray subscripting and the GDataXML layer) no longer allocate selector/class-name Strings per message; they now reject non-matching messages allocation-free.
- The guest-memory read/write fast path (`read`, `write`, `bytes_at`, `bytes_at_mut`, `ptr_at`, `ptr_at_mut`) is now marked `#[inline]` so callers in hot framework code don't pay a call and re-derivation each access.
- Implemented true frame pacing in the EAGL presentation path, making the picture noticeably smoother: the frame limiter now paces the guest to an exact frame deadline using a hybrid timer — a cooperative `env.sleep()` until shortly before the deadline (other guest threads still run) followed by a short spin-wait for the last ~2ms. Previously the pacing sleep was a plain timer sleep that could wake several ms late, starting the guest's next frame late and visibly wobbling the frame cadence (micro-stutter). The wake-up now lands within tens of microseconds of the deadline.
- The CPU scheduler batch is now adaptive: 1,000,000 ticks during long busy stretches, automatically reduced to 100,000 whenever any guest thread has an imminent wake deadline (<10ms), so frame pacing, run-loop timers and audio callbacks stay millisecond-precise.
- Major performance optimisation pass, focused on game FPS on Android devices:
  - The dynarmic CPU JIT now enables its "unsafe" floating-point/codegen optimizations (`Unsafe_UnfuseFMA`, `Unsafe_ReducedErrorFP`, `Unsafe_InaccurateNaN`, `Unsafe_IgnoreStandardFPCRValue`; the accuracy differences are limited to FP edge cases that games don't depend on), so VFP/NEON-heavy guest code no longer pays for per-instruction NaN/rounding bookkeeping where the backend supports these switches (currently the x86-64 JIT backend; on AArch64 hosts the flags are accepted but don't change emitted code yet). `Unsafe_IgnoreGlobalMonitor` deliberately stays off so guest atomics remain correct on multithreaded apps.
  - The CPU scheduler batch was raised from 100,000 to 1,000,000 ticks, amortising JIT exits, coroutine switches and scheduler passes ~10x better. Event polling keeps its independent 120 Hz throttle and guest thread wakeups are deadline-based, so input and timer responsiveness are unaffected.
  - The release profile now uses fat (whole-program) LTO instead of thin LTO, letting LLVM inline across all crate boundaries in the hottest emulation paths.
  - The global allocator is now mimalloc, which handles the emulator's small-allocation-heavy workload (autorelease pools, string and collection churn in the HLE frameworks) measurably faster than the platform allocator, especially on Android.
  - On Android the emulator thread's scheduling priority is raised via SDL's `SDL_SetThreadPriority(SDL_THREAD_PRIORITY_HIGH)`, keeping the emulation loop on big CPU cores on big.LITTLE SoCs.
  - The window framebuffer no longer requests depth/stencil buffers it never uses (everything host-drawn is a flat textured quad), saving a swap chain resolution's worth of bandwidth on tile-based mobile GPUs.
  - Per-frame/per-touch `getenv`-style debug toggle checks (`TOUCHHLE_*` env vars on the present, viewport, draw-call, hit-test and touch-remap paths) are now read once and cached; previously several of them ran an environ scan with locking and allocation on every frame or touch event.

## v0.2.3 (2026-01-02)

Compatibility:

- New working apps:
  - Dungeon Hunter (@ciciplusplus)
  - Crystal Defenders: Vanguard Storm (@ciciplusplus)
  - Zombie Infection (@ciciplusplus)
  - Gangstar: West Coast Hustle (@ciciplusplus)
  - Asphalt 4: Elite Racing (@ciciplusplus)
  - Prince of Persia: Warrior Within (@ciciplusplus)
  - Resident Evil 4: Mobile Edition (@alborrajo)
  - Command & Conquer: Red Alert (@ciciplusplus)
  - SimCity (@ciciplusplus)
  - Asphalt 5 (@ciciplusplus, @hikari-no-yume)
  - Cut the Rope (@ciciplusplus)
  - Skater Nation (@ciciplusplus)
  - Iron Man 2 (@ciciplusplus)
  - Shrek Forever After (@ciciplusplus)
  - Spore Origins (@ciciplusplus, @hikari-no-yume, @teromene)
  - Defender Chronicles (@hujerhoe)
  - Real Racing (@ciciplusplus)
  - Tom Clancy's Splinter Cell: Conviction (@ciciplusplus)
  - Assassin's Creed (@ciciplusplus)
  - N.O.V.A. Near Orbit Vanguard Alliance (@ciciplusplus)
  - Brothers in Arms 2: Global Front (@ciciplusplus)
  - Ferrari GT: Evolution (@ciciplusplus)
  - Castle Frenzy (@ciciplusplus)
  - Hero of Sparta 2 (@ciciplusplus)
  - Hero of Sparta (@ciciplusplus)
  - Bridge Odyssey (@ciciplusplus)
  - Terminator Salvation (@ciciplusplus)
  - Brothers In Arms: Hour Of Heroes (@ciciplusplus)
  - Crusade Of Destiny (@ciciplusplus)
  - Arvale (@ciciplusplus)
  - Battlefield: Bad Company 2 (@ciciplusplus)
  - Ms. PAC-MAN (@acieslewicz)
  - Dark Nebula (@ciciplusplus)
  - FIFA 10 (@ciciplusplus)
  - Crash Bandicoot Nitro Kart 2 (@ciciplusplus)
  - Driver (@ciciplusplus)
  - Sacred Odyssey: Rise of Ayden (@ciciplusplus)
  - Nanosaur 2 (@ciciplusplus)
  - Cro-Mag Rally (@ciciplusplus)
  - Bugdom 2 (@ciciplusplus)
- API support improvements:
  - Various small contributions. (@hikari-no-yume, @alborrajo, @ciciplusplus, @atasro2, @abnormalmaps, @hujerhoe, @acieslewicz, @WhatAmISupposedToPutHere, @JaGoTu, @apexad, @chyyran, @mistydemeo, @bognarit80, @RMZeroFour)
  - UITextField now supports real text input with a keyboard. On Windows/macOS physical keyboard is used, on Android it's done via a system soft keyboard. (@ciciplusplus)
  - UIScrollView and UITextView partial implementations. (@Skryptonyte, @ciciplusplus)
  - Core Animation and UIKit now support affine transforms, allowing UI elements to be rotated, a feature needed by [several games](https://github.com/touchHLE/touchHLE/issues/388). Note however that auto-rotation is not yet supported. (@hikari-no-yume)
  - Partial support for Core Animation explicit animations has been added. (@alborrajo)
  - The libz dynamic library is now available, [compiled from source](https://github.com/touchHLE/zlib-dylib) using a [clean open-source toolchain](https://github.com/touchHLE/common-3.0-sdk). (@acieslewicz)
  - ALAC and Microsoft IMA ADPCM are now supported in Audio Toolbox, with the same caveats as other compressed codecs. (@abnormalmaps)
  - Switched to ARMv7 rather than ARMv6 versions of libstdc++ and libgcc. (@acieslewicz)
  - Added support for certain iPhone OS 3.1 binary format changes (iPhone OS 3.1 apps are still considered unsupported). (@bognarit80)
  - Limited support for local multiplayer via Wi-Fi in some games. (@ciciplusplus)

Usability:

- The app picker now has an iOS-style “+” tile (the first icon in the grid) for adding a game. Tapping it lets the user pick an .ipa file, which is simply copied into the touchHLE_apps directory, and the grid then refreshes automatically. (@KlugKlugTG)
- Default options for various games have been added or improved. (@celerizer, @nighto)
- The app picker now has a “Quick options” feature. This provides a quicker and easier way to set some common options. (@hikari-no-yume)
- App icons in the app picker are now sorted by the display name of the app, case-insensitively. (@hikari-no-yume)
- The accelerometer (tilt controls) can now be simulated using a mouse, instead of a game controller or real accelerometer. Simply hold down the right mouse button and move the mouse cursor. (@alborrajo)
- The new `--disable-analog-stick-tilt-controls` option can be used to disable the use of the game controller's analog sticks for accelerometer simulation. This is useful on devices with both an integrated game controller and an integrated accelerometer, as touchHLE by default will only use the real accelerometer if no game controller is detected. (@hikari-no-yume)
- Android builds and releases of touchHLE now have an icon and meaningful version metadata. They also now use a different package name for preview builds versus releases, which means you can install them side-by-side. (@hikari-no-yume)
- macOS builds and releases of touchHLE now come as an application bundle (`.app` directory) rather than as a bare “Unix executable” file. This should fix problems some users encountered with running touchHLE outside of a terminal, and allows putting touchHLE in the Applications folder like a normal graphical app. To support this, user data (apps, options, etc) is now stored in “Application Support” rather than the current directory, and the bundled files (fonts, dylibs, etc) are now part of the app bundle. If you prefer the old layout, you can still get it if you move all the files out of the bundle. (@hikari-no-yume)
- The “File manager” button on Android now works more reliably, especially the first time it is tapped. (@hikari-no-yume)
- The new `--force-composition=` option has been added, which is a workaround that may solve rendering issues in some games, at the cost of performance. For some games it is applied by the default options. (@ciciplusplus)
- Most errors causing touchHLE to crash now produce a graphical message box, rather than the error message only being found in the log. (@abnormalmaps)
- touchHLE now writes log messages to a file on all platforms, not just on Android. The file has been renamed from `log.txt` to `touchHLE_log.txt`. (@hikari-no-yume)
- Two new options for input handling of analog stick (`--stick-to-touch=`) and 8-directional DPad (`--dpad-to-touch=`) via a game controller. (@celerizer)

Quality:

- Fixed an issue on some Android phones where the accelerometer was not usable. (@Oscar1640)
- Fixed multi-touch in some games. (@ciciplusplus)
- App icons are now displayed with a glossy sheen where required. (@hikari-no-yume)
- The app icons and labels in the app picker are now displayed at integer pixel offsets, making them sharper and more symmetrical. (@hikari-no-yume)

Other:

- MP3 decoding now uses Symphonia rather than dr\_mp3. We do not expect this to affect compatibility. (@abnormalmaps)

## v0.2.2 (2024-04-01)

Compatibility:

- New working apps:
  - Rayman 2 (@ciciplusplus)
  - Tony Hawk's Pro Skater 2 (@ciciplusplus)
  - Earthworm Jim (@ciciplusplus)
  - Castle of Magic (@ciciplusplus)
- API support improvements:
  - Various small contributions. (@alborrajo, @WhatAmISupposedToPutHere, @ciciplusplus, @hikari-no-yume, @abnormalmaps, @Skryptonyte, @teromene)
  - AAC audio files (AAC-LC in a typical MPEG-4 container) are now supported in Audio Toolbox. This is done in a fairly hacky way so it might not work for some apps. (@hikari-no-yume)
- There is now support for iPhone OS 3.0 apps, in addition to the existing support for iPhone OS 2.x apps:
  - Support for fat binaries has been added. touchHLE will no longer crash when trying to run an app with both ARMv6 and ARMv7 versions, and instead will try to pick the best available option (ARMv7, or failing this, ARMv6). This improves compatibility with iPhone OS 3.0 apps, many of which use fat binaries in order to improve performance on the iPhone 3GS and iPod touch (3rd generation). (@WhatAmISupposedToPutHere)
  - The bundled ARMv6 dynamic libraries, libgcc and libstdc++, have been updated to their iPhone OS 3.0.1 versions. Previously the iPhone OS 2.2.1 versions were used. (@hikari-no-yume)
  - touchHLE will no longer output a warning when trying to run an app with iPhone OS 3.0 as its minimum OS version. The warning now only appears for apps requiring iPhone OS 3.1 and later. (@hikari-no-yume)

Usability:

- The `--button-to-touch=` option now supports the Start and the LeftShoulder buttons in addition to the A/B/X/Y buttons and D-pad. Certain games' default options have been adjusted to use them. (@nighto)
- Default options for various games (@nighto)

## v0.2.1 (2023-10-31)

From this release onwards, the old list of supported apps is replaced by the crowdsourced touchHLE app compatibility database.

Compatibility:

- API support improvements:
  - Various small contributions. (@hikari-no-yume, @ciciplusplus, @alborrajo)
- New working apps:
  - Doom (@ciciplusplus)
  - Doom II RPG (@alborrajo)
  - I Love Katamari (@ciciplusplus)
  - Wolfenstein RPG (@alborrajo)

Quality:

- Multi-touch is now supported. (@ciciplusplus)

Usability:

- The Android version of touchHLE now has a _documents provider_. Thanks to a mere three hundred lines of boilerplate code [originally written for the emulator Skyline](https://github.com/skyline-emu/skyline/blob/dc20a615275f66bee20a4fd851ef0231daca4f14/app/src/main/java/emu/skyline/provider/DocumentsProvider.kt) (RIP), it is now possible for you, as the owner of a device running a newer Android version, to move ~~files~~ _documents_ in and out of touchHLE's ~~directory~~ _location_ on your device with relative ease. For example, it is now possible to download an ~~.ipa file~~ _`application/octet-stream` document_ to the Downloads folder of your device, then, using an appropriate app, move this _document_ to the touchHLE _location_. Users of normal operating systems and [older versions of Android](https://developer.android.com/about/versions/11/privacy/storage#other-apps-data) continue to be able to access a superior version of the same functionality via a so-called “file manager”. (@hikari-no-yume)
- There is now an “Open file manager” button in the app picker, to make it easier to find where touchHLE stores your apps and settings. On most operating systems this opens the relevant directory in a file manager, and on Android it opens some sort of app for managing _documents_ in the touchHLE _location_. (@hikari-no-yume)
- The Android version of touchHLE now writes all log messages to a file called `log.txt`, in addition to outputting them to logcat. (@hikari-no-yume)
- The new `--stabilize-virtual-cursor=` option makes the analog stick-controlled virtual cursor appear more stable to the emulated app, which is helpful in some games with overly sensitive menu scrolling. In some titles it is applied by default. (@hikari-no-yume; special thanks: @wareya)
- Automatic language detection now works on all platforms, and supports a list of languages in order of preference, rather than just one. The `LANG` environment variable is no longer supported, and instead the new `--preferred-languages=` option can be used. Note that it is the emulated app itself that decides what to do with this list, and whether particular languages are supported. (@hikari-no-yume)
- The app picker now has multiple pages, so it is no longer limited to 16 apps. (@hikari-no-yume)
- The framerate is now limited to 60fps by default, which matches the original iPhone OS and fixes issues with some games where the game ran too fast or consumed excessive energy and CPU time. This limit can be adjusted or disabled with the new `--limit-fps=` option. (@hikari-no-yume; special thanks: @wareya)
- The `--button-to-touch=` option now supports D-pad mappings in addition to the A/B/X/Y buttons. (@alborrajo)
- Default game controller button mappings have been added for Wolfenstein RPG and Doom II RPG, including for the D-pad. (@alborrajo)

## v0.2.0 (2023-08-31)

Compatibility:

- API support improvements:
  - Various small contributions. (@hikari-no-yume, @KiritoDv, @ciciplusplus, @TylerJaacks, @abnormalmaps)
  - PVRTC and paletted texture compression is now supported. (@hikari-no-yume)
  - Some key pieces of UIKit and Core Animation are now implemented: layer and view hierarchy, layer and view drawing, layer compositing, touch input hit testing, `UIImageView`, `UILabel`, `UIControl`, and `UIButton`. Previously, touchHLE could only support apps that draw everything with OpenGL ES, which is only common for games. This lays the groundwork for supporting games that rely on UIKit, and possibly some non-game apps. (@hikari-no-yume)
  - Threads can now sleep, join other threads, and block on mutexes. (@abnormalmaps, @hikari-no-yume)

- New supported apps:
  - Fastlane Street Racing (@hikari-no-yume)
  - Mystery Mania (@KiritoDv)
  - [Wolfenstein 3D](https://www.youtube.com/watch?v=omViNgUqF8c) (@ciciplusplus; version 1.0 only)
  - Many old apps by Donut Games (@ciciplusplus)

Quality and performance:

- Fixed opaque `COMPRESSED_RGB_PVRTC_*` textures rendering as a black screen (with working audio and touch input) on hosts that lack `GL_IMG_texture_compression_pvrtc` and therefore software-decode PVRTC to RGBA — for example Adreno/Mali devices and the GLES1-on-GL2 layer. Per the `IMG_texture_compression_pvrtc` spec the RGB variants have a base internal format of RGB, so their sampled alpha must be 1.0; the software decoder was instead keeping the PVRTC block's stray per-texel alpha and uploading as `GL_RGBA`, so apps that draw with `GL_BLEND` + `GL_SRC_ALPHA` blended their world away to nothing. The decoded alpha is now forced opaque (and uploaded with a matching `GL_RGB` base format) for the RGB variants, across all GLES backends.
- Overlapping characters in text now render correctly. (@Xertes0)
- touchHLE now avoids polling for events more often than 120Hz. Previously, it would sometimes poll many times more often than that, which could be very bad for performance. This change improves performance in basically all apps, though the effects on the supported apps from previous releases are fairly subtle. (@hikari-no-yume)
- The macOS-only memory leak of up to 0.4MB/s seems to have been fixed! (@hikari-no-yume)
- App icons are now displayed with rounded corners, even if the PNG file contains a square image. This is more accurate to what iPhone OS does. (@hikari-no-yume)
- The memory allocator is a lot faster now. (@hikari-no-yume)

New platform support:

- touchHLE is now available for Android. Only AArch64 devices are supported. (@ciciplusplus, @hikari-no-yume)

Usability:

- touchHLE now supports real accelerometer input on devices with a built-in accelerometer, such as phones and tablets. This is only used if no game controller is connected. (@hikari-no-yume)
- The options help text is now available as a file (`OPTIONS_HELP.txt`), so you don't have to use the command line to get a list of options. (@hikari-no-yume)
- The new `--fullscreen` option lets you display an app in fullscreen rather than in a window. This is independent of the internal resolution/scale hack and supports both upscaling and downscaling. (@hikari-no-yume)
- touchHLE now has a built-in app picker with a pretty icon grid. Specifying an app on the command line bypasses it. (@hikari-no-yume)
- The new `--button-to-touch=` option lets you map a button on your game controller to a point on the touch screen. touchHLE also now includes default button mappings for some games. (@hikari-no-yume)
- The new `--print-fps` option lets you monitor the framerate from the console. (@hikari-no-yume)

Other:

- To assist with debugging and development, touchHLE now has a primitive implementation of the GDB Remote Serial Protocol. GDB can connect to touchHLE over TCP and set software breakpoints, inspect memory and registers, step or continue execution, etc. This replaces the old `--breakpoint=` option, which is now removed. (@hikari-no-yume)
- The version of SDL2 used by touchHLE has been updated to 2.26.4. (@hikari-no-yume)
- Building on common Linux systems should now work without problems, and you can use dynamic linking for SDL2 and OpenAL if you prefer. Note that we are not providing release binaries. (@GeffDev)
- Some major changes have been made to how touchHLE interacts with graphics drivers:
  - touchHLE can now use a native OpenGL ES 1.1 driver where available, rather than translating to OpenGL 2.1. This is configurable with the new `--gles1=` option. (@hikari-no-yume)
  - The code for presenting rendered frames to the screen has been rewritten for compatibility with OpenGL ES 1.1. (@hikari-no-yume)
  - The splash screen is now drawn with OpenGL ES 1.1, either natively or via translation to OpenGL 2.1, rather than with OpenGL 3.2. (@hikari-no-yume)

  Theoretically, none of these changes should affect how touchHLE behaves for ordinary users in supported apps, but graphics drivers are inscrutable and frequently buggy beasts, so it's hard to be certain. As if to demonstrate this, these changes somehow fixed the mysterious macOS-only memory leak.
- The new `--headless` option lets you run touchHLE with no graphical output and no input whatsoever. This is only useful for command-line apps. (@hikari-no-yume)

## v0.1.2 (2023-03-07)

Compatibility:

- API support improvements:
  - Various small contributions. (@hikari-no-yume, @nitinseshadri)
  - Some key parts of `UIImage`, `CGImage` and `CGBitmapContext` used by Apple's `Texture2D` sample code are now implemented. Loading textures from PNG files in this way should now work. (@hikari-no-yume)
  - MP3 is now a supported audio file format in Audio Toolbox. This is done in a fairly hacky way so it might not work for some apps. (@hikari-no-yume)
- New supported apps:
  - Touch & Go LITE (@hikari-no-yume)
  - Touch & Go \[added to changelog after release: 2023-03-12\] (@hikari-no-yume)
  - Super Monkey Ball Lite (@hikari-no-yume; full version was already supported)

Quality:

- The version of stb\_image used by touchHLE has been updated. The new version includes a fix for a bug that caused many launch images (splash screens) and icons to fail to load. Thank you to @nothings and @rygorous who diagnosed and fixed this.

Usability:

- The virtual cursor controlled by the right analog stick now uses a larger portion of the analog stick's range. (@hikari-no-yume)
- Basic information about the app bundle, such as its name and version number, is now output when running an app. There is also a new command-line option, `--info`, which lets you get this information without running the app. (@hikari-no-yume)
- You are now warned if you try to run an app that requires a newer iPhone OS version. (@hikari-no-yume)
- Options can now be loaded from files. (@hikari-no-yume)
  - The recommended options for supported apps are now applied automatically. See the new `touchHLE_default_options.txt` file.
  - You can put your own options in the new `touchHLE_options.txt` file.
  - If you're a Windows user, this means that dragging and dropping an app onto `touchHLE.exe` is now all you need to do to run an app.

Other:

- The version of dynarmic used by touchHLE has been updated. This will fix build issues for some people. (@hikari-no-yume)

## v0.1.1 (2023-02-18)

Compatibility:

- API support improvements:
  - Various small contributions. (@hikari-no-yume, @nitinseshadri, @abnormalmaps, @RealSupremium)
  - Basic POSIX file I/O is now supported. Previously only standard C file I/O was supported. (@hikari-no-yume)
  - Very basic use of Audio Session Services is now supported. (@nitinseshadri)
  - Very basic use of `MPMoviePlayerController` is now supported. No actual video playback is implemented. (@hikari-no-yume)
- New supported app: Crash Bandicoot Nitro Kart 3D (@hikari-no-yume; version 1.0 only).

Quality and performance:

- The code that limits CPU use has reworked in an attempt to more effectively balance responsiveness and energy efficiency. Frame pacing should be more consistent and slowdowns should be less frequent. No obvious impact on energy use has been observed. (@hikari-no-yume)
- The emulated CPU can now access memory via a more direct, faster path. This can dramatically improve performance and reduce CPU/energy use, in some cases by as much as 25%. (@hikari-no-yume)
- Fixed missing gamma encoding/decoding when rendering text using `UIStringDrawing`. This was making the text in _Super Monkey Ball_'s options menu look pretty ugly. (@hikari-no-yume)

Usability:

- `.ipa` files can now be opened directly, you don't need to extract the `.app` first. (@DCNick3)
- New command-line options `--landscape-left` and `--landscape-right` let you change the initial orientation of the device. (@hikari-no-yume)
- The app bundle or `.ipa` file no longer has to be the first command-line argument. (@hikari-no-yume)

Other:

- Some of the more spammy warning messages have been removed or condensed. (@hikari-no-yume)

## v0.1.0 (2023-02-02)

First release.
