# FFmpeg movie playback backend

HyperHLE supports real movie playback via FFmpeg behind the optional
`ffmpeg` Cargo feature. The default build does **not** require FFmpeg
development headers or libraries and falls back to the legacy "skip
cut-scene after 150 ms" stub.

This document covers:

1. What the `ffmpeg` feature actually does
2. How to enable it on Windows
3. How to enable it for Android (`cargo-ndk` + prebuilt FFmpeg)
4. Smoke-testing the backend on the host
5. Known limitations and risks

## 1. What the feature does

Enabling `--features ffmpeg`:

* Adds [`ffmpeg-next`](https://crates.io/crates/ffmpeg-next) 7.1 (which
  pulls `ffmpeg-sys-next` transitively) to the dependency graph.
* Compiles `src/media/ffmpeg_backend.rs` and `src/media/ffmpeg_pipeline.rs`.
* Causes `media::open_movie()` to attempt real demux + decode of any
  movie file passed to `MPMoviePlayerController initWithContentURL:`.
  Decoded video is converted to RGBA8, decoded audio is converted to
  interleaved S16 stereo. Both flow into bounded
  `std::sync::mpsc::sync_channel` queues owned by a dedicated worker
  thread.
* Leaves the legacy fast-finish stub in place as a fallback: if FFmpeg
  cannot open the supplied file (missing codec, exotic container,
  broken header) the host code falls back to `NullBackend` and behaves
  exactly like the default build.

Without `--features ffmpeg`, `ffmpeg-next` is **not** in the build graph
at all – `cargo check` and `cargo build` work on a vanilla toolchain
without any system FFmpeg.

## 2. Windows

Recommended approach: **vcpkg** with `x64-windows-static-md` triplet
(matches the default MSVC `/MD` runtime that `cargo` uses).

```powershell
git clone https://github.com/Microsoft/vcpkg.git
cd vcpkg
.\bootstrap-vcpkg.bat
.\vcpkg install ffmpeg:x64-windows-static-md
$env:VCPKG_ROOT = "$pwd"
$env:FFMPEG_DIR = "$pwd\installed\x64-windows-static-md"
$env:PKG_CONFIG_PATH = "$env:FFMPEG_DIR\lib\pkgconfig"
```

Then, from the repository root:

```powershell
cargo build --release --features ffmpeg
```

If you prefer the prebuilt BtbN binaries
(<https://github.com/BtbN/FFmpeg-Builds/releases>):

1. Download a `ffmpeg-master-latest-win64-gpl-shared.zip` build.
2. Extract somewhere stable, e.g. `C:\dev\ffmpeg-7.1`.
3. Set `FFMPEG_DIR=C:\dev\ffmpeg-7.1` and prepend
   `C:\dev\ffmpeg-7.1\bin` to `PATH` (needed at runtime for the
   `avcodec-XX.dll` / `avformat-XX.dll` / ... DLLs).

`ffmpeg-sys-next` uses `pkg-config` first and falls back to
`FFMPEG_DIR` if `pkg-config` is missing.

## 3. Android

We do not ship a copy of FFmpeg inside the repo; instead we link
against a prebuilt set of static libraries that you supply via
`FFMPEG_DIR` per target ABI.

### One-time toolchain setup

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  x86_64-linux-android i686-linux-android
cargo install cargo-ndk
```

Install Android NDK r25c or later and export `ANDROID_NDK_HOME`.

### Obtaining FFmpeg static libs

The easiest path is the
[`tanersener/ffmpeg-kit`](https://github.com/tanersener/ffmpeg-kit)
prebuilt packages, which already cover the four Android ABIs above and
ship as a tar.gz per architecture. After unpacking, expect a layout
like:

```
android-prebuilt/
  arm64-v8a/
    include/
    lib/
      libavcodec.a
      libavformat.a
      libavutil.a
      libswresample.a
      libswscale.a
      pkgconfig/*.pc
  armeabi-v7a/...
  x86/...
  x86_64/...
```

### Building

```bash
export ANDROID_NDK_HOME=/opt/android-ndk-r26d
export FFMPEG_DIR=/path/to/android-prebuilt/arm64-v8a
export PKG_CONFIG_PATH="$FFMPEG_DIR/lib/pkgconfig"
export PKG_CONFIG_ALLOW_CROSS=1

cargo ndk -t arm64-v8a -p 24 build --release --features ffmpeg
```

Repeat with `-t armeabi-v7a / x86 / x86_64` and the matching
`FFMPEG_DIR` for each ABI you ship.

Notes:

* `bindgen` (used by `ffmpeg-sys-next`) needs the NDK's `clang`. With
  `cargo-ndk` this is set automatically – do not override `CC`/`CXX`.
* If you compile FFmpeg yourself, pass at minimum
  `--enable-cross-compile --target-os=android --arch=aarch64
  --sysroot=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST/sysroot
  --cc=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST/bin/aarch64-linux-android24-clang
  --disable-programs --disable-doc --disable-everything
  --enable-decoder=h264,mpeg4,aac,mp3,vorbis
  --enable-demuxer=mov,mp4,m4a,matroska,webm
  --enable-protocol=file --enable-parser=h264,aac --enable-swscale
  --enable-swresample` to keep the libs small (≈ 6 MB / ABI).

## 4. Smoke test on host

```bash
cargo run --example ffmpeg_smoke --features ffmpeg -- path/to/movie.mp4
```

The example opens the supplied file, polls the backend for ~30
seconds (or until end-of-stream), and prints a compact summary of how
many frames / audio chunks were decoded. Use it as a sanity check that
your FFmpeg link is wired up correctly before plugging an `.ipa` into
the emulator.

## 5. Risks and limitations

* **Decoded frames and samples are currently dropped by the
  `MPMoviePlayerController` integration.** The pipeline runs end-to-end
  (decode + colour convert + resample) and the
  `MPMoviePlayerPlaybackDidFinish` notification fires on real
  end-of-stream, but the frames are not yet uploaded into a UIKit view
  and the audio is not yet routed into the existing OpenAL output. That
  wiring is intentionally split out into a follow-up change – this
  module is the foundation it needs.
* **A/V sync is host-monotonic-clock-driven, not audio-master.** Good
  enough for short intro videos, which are the only thing
  `MPMoviePlayerController` is used for in the target catalogue. For
  long-form playback, override `FfmpegBackend::current_time_seconds()`
  with a clock fed by the audio device.
* **Container support is whatever FFmpeg was compiled with.** The
  default vcpkg / ffmpeg-kit builds cover MP4 / MOV / MKV / WebM and
  H.264 / MPEG-4 / AAC / MP3 / Vorbis. Apple-proprietary HE-AAC v2 in
  M4V works only with non-Free `libfdk_aac`; if a particular game
  needs that, document the GPL/non-free build flag in your own deploy
  notes.
* **Threading model is one decoder thread per player.** Multiple
  concurrent `MPMoviePlayerController` instances therefore each get
  their own thread. Not an issue in practice (games create at most one
  player), but worth knowing.
* **No hardware acceleration.** All decode happens on the CPU. For the
  240p–480p intro videos shipped with iPhone OS 1.x–3.x games this is
  negligible; if you ever target 1080p+ content, plug in
  `h264_mediacodec` (Android) / `h264_dxva2` (Windows) by passing the
  matching `Codec` to `ffmpeg::codec::context::Context::decoder()`.
* **`ffmpeg-sys-next` runs `pkg-config` and `bindgen` at build time.**
  Initial `cargo build --features ffmpeg` takes substantially longer
  than the default build and pulls in `libclang` as a system
  dependency.
