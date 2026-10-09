# HyperHLE-Fork

**HyperHLE-Fork** is a community-maintained fork of [HyperHLE](https://github.com/HyperHLE/HyperHLE), itself based on [touchHLE](https://github.com/touchHLE/touchHLE). The user-facing name of this project is **HyperHLE**.

HyperHLE is a high-level emulator for early iPhone OS apps. It reimplements selected iOS frameworks on the host, allowing compatible apps to run without booting iOS. This fork focuses on Android/mobile usability, fullscreen and orientation handling, graphics compatibility, and app-specific fixes. It is not an official Apple product.

The repository does not distribute commercial apps or iOS firmware. Put an app bundle (`.app`) or IPA (`.ipa`) that you are entitled to use in `touchHLE_apps/`; it will appear in the app picker.

## Platforms and features

- Builds for Android, Windows, macOS, and Linux.
- Virtual iPhone and iPad device profiles.
- Android touch input and fullscreen/orientation handling.
- Ongoing GLES, EAGL, Core Animation, audio, and iOS framework compatibility work.
- Download build artifacts from the [HyperHLE GitHub Actions workflow](https://github.com/KlugKlugTG/HyperHLE-Fork/actions/workflows/HyperHLE_release.yml).

## Newly tested and working games

The titles below have been tested with HyperHLE-Fork and are known to run. Compatibility may still vary with the app version, virtual device profile, host GPU/driver, and settings; this list does not guarantee that every feature or a full playthrough works.

- N.O.V.A. 3 (Playable)
- Gangstar Vegas (Unplayable)
- Geometry Dash (2.11 and 1.0 + playable)
- Terraria (1.0 + playable)
- Modern Combat 3 (1.5.0 + playable)
- Asphalt 7 (Playable)
- Turbo Dismount (Pretty playable)
- Scarface (Playable)
- Zombie Safari (Playable)
- Real Racing 1 (Playable)
- Silent Ops (Playable)
- Need For Speed: Most Wanted (Unplayable)
- Minecraft 0.14.2-0.16.2 (Playable)
- Oceanhorn (Unplayable)
- Asphalt 8 (1.0.0 + Playable)
- GTA 3 (Buggy)
- And more more games

## Build and documentation

Build and run a desktop version with Cargo:

```sh
cargo run --release -- path/to/app.app
```

For Android build steps, see the `android/` directory and the [CI workflow](https://github.com/KlugKlugTG/HyperHLE-Fork/actions/workflows/HyperHLE_release.yml). Command-line options are documented in [`OPTIONS_HELP.txt`](OPTIONS_HELP.txt); notable compatibility changes are tracked in [`CHANGELOG.md`](CHANGELOG.md).

## Community

Join the [HyperHLE Discord server](https://discord.gg/ZpEkAV47H9) to discuss the project and contribute.

## Credits and license

HyperHLE-Fork carries forward work from HyperHLE and touchHLE contributors and uses open-source libraries. It is licensed under the [Mozilla Public License 2.0](LICENSE); see the individual projects for their respective notices and licenses.
