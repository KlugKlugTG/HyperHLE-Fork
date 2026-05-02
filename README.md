# HyperHLE: high-level emulator for iPhone OS apps

**HyperHLE** is a community-driven, high-level emulator for early iPhone OS
apps (iPhone OS 2.x / 3.x). It runs on Windows, macOS, Linux and Android, and
is written in Rust.

HyperHLE is a fork of the original [touchHLE](https://github.com/touchHLE/touchHLE)
project. We built on its solid HLE foundation and steered the emulator towards
a more aggressive triage workflow, broader app compatibility, automated
crash-diagnostic pipelines, and our own crowdsourced app-compatibility
database. All of HyperHLE remains MPL-2.0 / GPL-3.0+ licensed and open to
contribution. Original touchHLE work is credited in the **License** and
**Thanks** sections below.

HyperHLE's high-level emulation (HLE) approach differs from low-level emulation
(LLE) in that it does not directly simulate the iPhone/iPod touch hardware.
Instead of running iPhone OS inside emulation, HyperHLE _itself_ takes the
place of iPhone OS and provides its own implementations of the system
frameworks (Foundation, UIKit, OpenGL ES, OpenAL, etc). The only code the
[emulated CPU](https://github.com/merryhime/dynarmic) executes is the app
binary and [a handful of libraries](HyperHLE_dylibs/).

The goal of this project is to run games from the early days of iOS:

* Currently: iPhone and iPod touch apps for iPhone OS 2.x and iPhone OS 3.0.
* Longer term: iPhone OS 3.1, iPad apps (iPhone OS 3.2), iOS 4.x, …
* Never: 64-bit iOS.

**Most apps for these OS versions still don't work.** The vast majority of
iPhone OS 2.x and 3.x apps do not currently run in HyperHLE; the ones that do
are mostly games. Compatibility improves gradually as people contribute fixes.
The [HyperHLE app compatibility database](https://hyperhle-appdb-kupykrhh.fly.dev/)
tracks which apps work; it's a crowdsourced effort and anyone can contribute.
We don't take requests, so please don't ask us to support a specific game.

## Links

* **Latest fork build (CI artifact):** <https://github.com/j92580498-max/HyperHLE/actions/runs/25253048200>
* **Telegram community:** <https://t.me/shevahle>
* **Official HyperHLE app database:** <https://hyperhle-appdb-kupykrhh.fly.dev/>
* **Source / issues:** <https://github.com/j92580498-max/HyperHLE>

## Important disclaimer

This project is not affiliated with or endorsed by Apple Inc. iPhone, iOS,
iPod, iPod touch and iPad are trademarks of Apple Inc in the United States and
other countries.

Only use HyperHLE to emulate software you have obtained legally.

## Platform support

* Officially supported: x64 Windows, x64 macOS and AArch64 Android.
  * These are the platforms with binary releases via our
    [GitHub Actions CI](https://github.com/j92580498-max/HyperHLE/actions).
  * On Apple Silicon Macs, the x64 build reportedly works under Rosetta.
* Probably works, but you must build it yourself: AArch64 macOS, x64 Linux,
  AArch64 Linux.
* Never?: other architectures.

Input methods:

- For simulated touch input, there are four options:
  - Mouse/trackpad input (tap/hold/drag with the left mouse button).
  - Virtual cursor using a game controller (move with the right analog stick,
    tap/hold/drag with the stick or right shoulder button).
  - Mapping of game controller buttons or the left analog stick to specific
    on-screen locations (see `--button-to-touch=`, `--dpad-to-touch=` and
    `--stick-to-touch=` in `OPTIONS_HELP.txt`).
  - Real touch input on devices with a touch screen.
- For simulated accelerometer input, there are three options:
  - Tilt simulation via the left analog stick of a game controller.
  - Tilt simulation via mouse (hold the right mouse button).
  - Real accelerometer input on devices with one (e.g. phones, tablets).

## History of the HyperHLE fork

HyperHLE didn't appear out of nowhere. Here's how the fork came to be.

* **2022-12 – 2023-02 — touchHLE upstream is born.** The original touchHLE
  was built from scratch by [hikari\_no\_yume](https://hikari.noyu.me/) as a
  Rust-based HLE emulator for early iPhone OS apps and
  [released publicly in February 2023](https://hikari.noyu.me/blog/2023-02-06-touchhle-anouncement-thread-tech-games-me-and-passion-projects.html).
  Other volunteers gradually started contributing.
* **2023 – 2024 — touchHLE matures.** Compatibility expands, CI is set up,
  Android port is added, dynarmic JIT is integrated, the official
  appdb.touchhle.org compatibility database goes live.
* **2025 — fork rationale.** We wanted a more aggressive iteration loop than
  upstream's Gerrit workflow comfortably allowed: faster merging of triage
  fixes for popular early-iOS games, automated crash-log triage, and our own
  community space (Telegram, Russian-language docs, our own appdb instance).
  Rather than push the upstream into our shape, we forked.
* **2025-Q4 — HyperHLE is created.** The fork is published at
  [j92580498-max/HyperHLE](https://github.com/j92580498-max/HyperHLE) on top
  of touchHLE's `trunk` and immediately starts diverging:
  * Aggressive merging of fixes for Plants vs. Zombies, NOVA, Asphalt 4,
    Farm Frenzy, Red Ball, Doodle Jump, MCPE 0.8.x and similar early-iOS
    games (see `CHANGELOG.md`).
  * Automated triage pipelines that turn user-submitted crash logs into
    targeted patches.
  * Our own [HyperHLE app database](https://hyperhle-appdb-kupykrhh.fly.dev/)
    instance with screenshot upload, log autofill, GitHub OAuth login and
    admin moderation.
  * Telegram community at <https://t.me/shevahle> for support and builds.
* **Today.** HyperHLE tracks selected upstream changes manually and ships
  experimental builds via CI. We continue to credit upstream touchHLE
  authors; HyperHLE is a derivative work, not a replacement of their effort.

If you want a deeper technical primer on how the underlying high-level
emulation works, the original
[_touchHLE in depth_](https://hikari.noyu.me/blog/2023-04-13-touchhle-in-depth-1-function-calls.html)
write-up is still the best read.

# Usage

First obtain HyperHLE — either grab the
[latest CI build](https://github.com/j92580498-max/HyperHLE/actions/runs/25253048200)
or build it yourself (see the next section).

Then you'll need an app that you can run. The
[HyperHLE app compatibility database](https://hyperhle-appdb-kupykrhh.fly.dev/)
is a good guide for which app versions are known to work, but bear in mind
the data may be outdated or inaccurate. Note that the app binary must be
decrypted to be usable.

There's a few ways you can run an app in HyperHLE.

## Special Android notes

Windows, Mac and Linux users can skip this section.

On Android, only the graphical user interface (app picker) is available.
Therefore, you must put your “.ipa” files or “.app” bundles inside the
`HyperHLE_apps` directory. Note that you can only do that once you have run
HyperHLE at least once.

File management can be tricky on Android due to
[restrictions introduced by Google in newer Android versions](https://developer.android.com/about/versions/11/privacy/storage#scoped-storage).
One of these methods may work:

* If you tap the “File manager” button in HyperHLE, this should open some
  sort of file manager. You might also be able to find HyperHLE in your
  device's file manager app (often called “Files”, or sometimes “Downloads”),
  alongside cloud storage services. There are some limitations on what
  kinds of operations are possible. The files in this location are stored
  on your device. Warning: on some devices the “File manager” button _will_
  open a file manager but it will crash when actually doing file operations
  (probably an Android bug). If this happens, clear that file manager from
  your recent apps list and navigate to your device's file manager app
  directly.
* On older Android versions you may be able to directly access HyperHLE's
  files at `/sdcard/Android/data/org.hyperhle.android/files/HyperHLE_apps`.
  (`/sdcard` is usually not on the SD card, despite the name.)
* You may be able to use ADB. If you're unfamiliar with it, try
  <https://yume-chan.github.io/ya-webadb/> in Chrome (with WebUSB) over
  USB. HyperHLE's files are at `sdcard` > `Android` > `data` >
  `org.hyperhle.android` > `files` > `HyperHLE_apps`.

## Graphical user interface

HyperHLE has a built-in app picker. If you put your `.ipa` files and `.app`
bundles in the `HyperHLE_apps` directory, they show up in the picker when
you run HyperHLE.

To configure options, edit `HyperHLE_options.txt`. For a list of options,
see `OPTIONS_HELP.txt`.

## Command-line user interface

**This section does not apply on Android.**

You can see the command-line usage by passing the `--help` flag.

If you're a Windows user and unfamiliar with the command line:

1. Move the `.ipa` file or `.app` bundle to the same folder as `HyperHLE.exe`.
2. Hold Shift and right-click empty space in the folder window.
3. Click “Open with PowerShell”.
4. Type `.\HyperHLE.exe "YourAppNameHere.ipa"` (or `.app`) and press Enter.
   To pass options, add a space after the app name (outside the quotes) and
   list options separated by spaces.

## Local multiplayer support

HyperHLE provides limited support for local multiplayer over Wi-Fi in some
games. At the moment of writing it works in Asphalt 4 and N.O.V.A. Real iOS
devices can also join/host games.

**Usage:**
1. Install HyperHLE on 2+ devices on the same Wi-Fi network.
2. **Important:** Whitelist HyperHLE in your OS firewall/network settings.
3. Enable “Network access” in Quick options or via `--allow-network-access`.
4. Start/join multiplayer in the game.

**FAQs:**
* **Tunneling over Internet/VPN:** Not officially supported, may work.
* **Bluetooth:** Not supported.

**Known issues:**
* On macOS you may need to launch HyperHLE from the terminal, otherwise the
  OS may block network connections.

## Other stuff

Any data saved by an app (e.g. **saved games**) is stored in the
`HyperHLE_sandbox` folder.

If the emulator crashes almost immediately while running a **known-working**
version of a game, check whether you have any overlays turned on (Steam
overlay, Discord overlay, RivaTuner Statistics Server, etc). These tools
inject themselves into other apps and don't always clean up after
themselves, so they can break HyperHLE — it's not our fault. 😢 Currently
RivaTuner Statistics Server is known to be a problem. If you find another
overlay that breaks HyperHLE, tell us in our [Telegram](https://t.me/shevahle).

# Building and contributing

See the `CONTRIBUTING.md` file in the git repo if you want to contribute. If
you just want to build HyperHLE, look at `dev-docs/building.md`.

# License

HyperHLE is a fork of touchHLE.

* Original work © 2023–2026 touchHLE project contributors. Original sources
  at <https://github.com/touchHLE/touchHLE>.
* HyperHLE fork modifications © 2025–2026 HyperHLE project contributors and
  contributors to <https://github.com/j92580498-max/HyperHLE>.

The source code of HyperHLE (not its dependencies) is licensed under the
Mozilla Public License, version 2.0.

Due to license compatibility concerns, distributed binaries are under the
GNU General Public License version 3 or later.

For a best-effort listing of all dependency licenses, build HyperHLE and pass
the `--copyright` flag when running it, or click the “Copyright info” button
in the app picker.

Different licensing terms apply to the bundled dynamic libraries (in
`HyperHLE_dylibs/`) and fonts (in `HyperHLE_fonts/`). Consult the respective
directories.

# Thanks

We stand on the shoulders of giants. Thank you to:

* The original **touchHLE** authors and contributors — without their work
  HyperHLE would not exist. In particular [hikari\_no\_yume](https://hikari.noyu.me/),
  who started touchHLE in December 2022, and the
  [touchHLE contributors](https://github.com/touchHLE/touchHLE/graphs/contributors).
* Everyone who has contributed to HyperHLE or supported any of its
  contributors financially.
* The authors of and contributors to the many libraries used by this
  project: [dynarmic](https://github.com/merryhime/dynarmic),
  [rust-macho](https://github.com/flier/rust-macho), [SDL](https://libsdl.org/),
  [rust-sdl3](https://github.com/vhspace/sdl3-rs),
  [stb\_image](https://github.com/nothings/stb), Imagination Technologies'
  [PVRTC decompressor](https://github.com/powervr-graphics/Native_SDK/blob/master/framework/PVRCore/texture/PVRTDecompress.cpp),
  [openal-soft](https://github.com/kcat/openal-soft),
  [hound](https://github.com/ruuda/hound),
  [caf](https://github.com/rustaudio/caf),
  [Symphonia](https://github.com/pdeljanov/Symphonia),
  [RustType](https://gitlab.redox-os.org/redox-os/rusttype),
  [the Liberation fonts](https://github.com/liberationfonts/liberation-fonts),
  [the Noto CJK fonts](https://github.com/googlefonts/noto-cjk),
  [rust-plist](https://github.com/ebarnard/rust-plist),
  [nibarchive](https://github.com/michaelwright235/nibarchive),
  [quick-xml](https://github.com/tafia/quick-xml),
  [gl-rs](https://github.com/brendanzab/gl-rs),
  [cargo-license](https://github.com/onur/cargo-license),
  [cc-rs](https://github.com/rust-lang/cc-rs),
  [cmake-rs](https://github.com/rust-lang/cmake-rs),
  [cargo-ndk](https://github.com/bbqsrc/cargo-ndk),
  [cargo-ndk-android-gradle](https://github.com/willir/cargo-ndk-android-gradle),
  [md-5 and sha1](https://github.com/RustCrypto/hashes),
  [yore](https://github.com/bonega/yore),
  [encoding_rs](https://github.com/hsivonen/encoding_rs),
  [corosensei](https://github.com/Amanieu/corosensei) and the Rust standard
  library.
* The Skyline emulator project (RIP), for [writing the tedious boilerplate
  needed to replace file management on newer Android versions](https://github.com/skyline-emu/skyline/blob/dc20a615275f66bee20a4fd851ef0231daca4f14/app/src/main/java/emu/skyline/provider/DocumentsProvider.kt).
* The [Rust project](https://www.rust-lang.org/).
* The various people who've documented the iPhone OS platform — much of
  that documentation is linked from inside this codebase.
* The iOS hacking/jailbreaking community.
* The Free Software Foundation, for making libgcc and libstdc++ copyleft and
  thus saving this project from ABI hell.
* The National Security Agency of the United States of America, for
  [Ghidra](https://ghidra-sre.org/).
* Many friends who took an interest in the project and gave suggestions
  and encouragement.
* Developers of early iPhone OS apps. What treasures you created!
* Apple, and NeXT before them, for creating such fantastic platforms.
