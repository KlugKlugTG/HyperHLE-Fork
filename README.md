<div align="center">

<img src="res/icon.png" width="150" alt="HyperHLE icon">

# HyperHLE

### A high-level emulator that lets you replay the early days of the iPhone — on the devices you already own.

**iPhone OS 2.0 → iOS 8.0  ·  Windows · macOS · Linux · Android  ·  Written in Rust**

</div>

---

## ⭐ Why HyperHLE is so good

HyperHLE isn't trying to be a museum piece — it's built to **actually run your games, smoothly, today.** A few things make it special:

- **It's fast, because it doesn't waste effort.** Most emulators boot an entire operating system inside a virtual machine and pay for it on every frame. HyperHLE skips all of that: it *replaces* iPhone OS instead of simulating it. The only thing running on the emulated ARM CPU (powered by the excellent [dynarmic](https://github.com/merryhime/dynarmic) JIT) is the app itself. Everything else is native, modern host code. That's why startup is near-instant and frame rates stay high even on a phone.
- **It's genuinely broad.** This is not a one-or-two-games demo. HyperHLE ships **46 reimplemented system frameworks**, over **300 Objective-C classes**, and more than **2,600 system functions** — covering graphics, audio, touch, networking, location, motion, in-app purchases, Game Center and more.
- **It spans the whole golden era.** From the very first 2008-era apps up through the iOS 7/8 generation, HyperHLE understands both the legacy (`CFBundleIconFile`, single-orientation plists) and the modern (`CFBundleIcons`, Retina assets, interface-orientation arrays) ways apps were built.
- **It runs everywhere you do.** The same emulator core powers desktop *and* Android, so the phone in your pocket can play the games that phones used to play.
- **It looks and feels right.** A built-in home screen with the classic iOS wallpaper, proper icon rendering with the iconic rounded-corner sheen, real controller support, and tilt controls that map a gamepad's analog stick to the accelerometer.

If you have a legally-obtained copy of an old favourite, HyperHLE is one of the most pleasant ways there is to play it again.

---

## What is HyperHLE?

**HyperHLE** is a high-level emulator (HLE) for iPhone and iPod touch applications. It runs on modern desktop operating systems and Android, and is written in Rust for speed, portability and safety.

HyperHLE's high-level approach differs from low-level emulation (LLE) in a fundamental way. An LLE emulator directly simulates the iPhone hardware and runs a real copy of iPhone OS on top of it. HyperHLE does the opposite: it **takes the place of iPhone OS itself** and provides its own clean implementations of the system frameworks an app expects — Foundation, UIKit, OpenGL ES, OpenAL, Core Graphics, Core Animation and dozens more. The only code the [emulated CPU](https://github.com/merryhime/dynarmic) ever executes is the app binary and [a small handful of bundled libraries](HyperHLE_dylibs/).

This is what makes HyperHLE lightweight, quick to launch, and accurate for the era it targets — you get the app's real behaviour without the overhead of booting a whole OS.

> HyperHLE is a 2026 project. It builds on, and gives full credit to, the open-source [touchHLE](https://github.com/touchHLE/touchHLE) emulator, extending its framework coverage to span a much wider range of iPhone OS / iOS releases.

---

## Supported iOS versions

HyperHLE targets the first seven years of the platform:

| Era | Versions | Notes |
| --- | --- | --- |
| Classic iPhone OS | 2.0 – 3.2 | The original App Store generation; single-icon, single-orientation apps. |
| Early iOS | 4.0 – 5.1 | Retina assets, multitasking-era apps, `CFBundleIcons`. |
| Late support | 6.0 – 8.0 | Modern framework usage (Social, Core Image, Game Controller, Map Kit, etc.). |

Coverage of any given app depends on which APIs it uses — HyperHLE implements an enormous slice of the system, but not literally every method of every framework. Games and self-contained apps from this era are the sweet spot.

> **This project is not affiliated with or endorsed by Apple Inc. in any way.** iPhone, iOS, iPod, iPod touch and iPad are trademarks of Apple Inc. in the United States and other countries. **Only use HyperHLE to emulate software you have obtained legally.**

---

## What's inside (frameworks & features)

HyperHLE reimplements **46 frameworks**, including:

- **Graphics** — OpenGL ES 1.1 *and* OpenGL ES 2.0 (both native pass-through and translation-to-desktop-GL backends), Core Graphics, Core Animation, Core Image, Core Video, Quartz/CALayer compositing.
- **UI** — UIKit (views, controls, scroll views, view controllers, alerts, text), with a real touch/input model.
- **Audio** — OpenAL, Audio Toolbox, Core Audio, AVFoundation, Media Player.
- **Input & sensors** — multi-touch, accelerometer, Core Motion, Game Controller, with gamepad-to-tilt mapping.
- **Connectivity & services** — CFNetwork, Captive Network, System Configuration, Security / Common Crypto, Game Kit (Game Center), Store Kit (in-app purchases), Core Location, Map Kit, Address Book, Social / Twitter compose, Message UI, Store, accounts.
- **System libraries** — a large portion of libc/POSIX, libxml2, libsqlite3, libicucore, libbz2, the Objective-C runtime, and the dynamic linker (dyld).

Behind the scenes: **300+ Objective-C classes** and **2,600+ exported C functions**.

---

## Platform support

- **Officially supported (binary releases):** x64 Windows, x64 macOS, AArch64 Android.
  - On Apple Silicon Macs, the x64 build runs under Rosetta.
- **Builds yourself, generally works:** AArch64 macOS, x64 Linux, AArch64 Linux.
- **Device families:** iPhone, iPhone 5, and iPad — selectable with `--device-family=`, otherwise deduced from the app bundle.

---

## Getting started

1. Get a copy of HyperHLE for your platform (a release binary, or build it yourself — see below).
2. Put your **legally-obtained** `.ipa` files (or unpacked `.app` bundles) into the **`HyperHLE_apps`** folder next to the emulator. On Android, use the in-app file manager to copy apps into the app's data folder.
3. Launch HyperHLE. You'll land on the home screen — pick your app and play.

A few useful details:

- **Saved games and app data** live in the **`HyperHLE_sandbox`** folder.
- **Options:** run with `--help` to see every flag, or put options in `HyperHLE_options.txt`. Common ones: `--fullscreen`, `--landscape-left` / `--landscape-right`, `--scale-hack=2` (sharper internal resolution), `--device-family=ipad`, and the controller/tilt tuning flags (`--deadzone=`, `--x-tilt-range=`, `--button-to-touch=`, …).
- **Home screen wallpaper:** HyperHLE ships with the classic iOS wallpaper by default. To use your own, drop a `HyperHLE_wallpaper.png` / `.jpg` / `.jpeg` into the data folder.

### Controls

- **Touch:** mouse / trackpad (left button), or a touchscreen on Android.
- **Tilt / accelerometer:** a connected game controller's analog stick (or the right mouse button + cursor) simulates tilting the device. Tunable via the tilt options.
- **Buttons:** game controller buttons can be mapped to on-screen touch points with `--button-to-touch=`.

### A note on crashes

If the emulator crashes almost immediately on a *known-working* game, check for screen-overlay tools (Steam overlay, Discord overlay, RivaTuner Statistics Server, etc.). These inject themselves into other programs and can break HyperHLE — it isn't HyperHLE's fault. RivaTuner Statistics Server is the known offender; if you find another, please report it.

---

## Building & contributing

You'll need [git](https://git-scm.com/), the [Rust toolchain](https://www.rust-lang.org/tools/install), [CMake](https://cmake.org/), and your platform's standard C and C++ compilers. Then:

```sh
cargo run --release      # release build (recommended)
cargo run                # debug build
```

A clean release build takes only a few minutes on a modest machine. For Android, dynamic-linking, and cross-compilation notes, see [`dev-docs/building.md`](dev-docs/building.md). If you'd like to contribute, read [`CONTRIBUTING.md`](CONTRIBUTING.md) first.

---

## License

HyperHLE is a 2026 project, built on **touchHLE © 2023–2026 touchHLE project contributors**.

The source code of HyperHLE / touchHLE itself (not its dependencies) is licensed under the **Mozilla Public License, version 2.0**. Some bundled components are under other licenses — most notably parts of libgcc/libstdc++ (GPLv3-or-later with the runtime exception). Different terms apply to the bundled dynamic libraries (`HyperHLE_dylibs/`) and fonts (`HyperHLE_fonts/`); see those directories for details. For a full best-effort listing of dependency licenses, build the emulator and run it with the `--copyright` flag.

---

## Thanks

HyperHLE stands on the shoulders of giants. Thank you to:

- The **touchHLE** project and all of its contributors — HyperHLE would not exist without your work.
- The authors of [dynarmic](https://github.com/merryhime/dynarmic), and the wider [Rust](https://www.rust-lang.org/) community.
- Everyone who has documented the iPhone OS platform, officially or otherwise, and the iOS hacking / jailbreaking community.
- The developers of those early iPhone OS apps and games — what treasures you created!
- Apple, and NeXT before them, for building such fantastic platforms.
