# HyperHLE

HyperHLE is an independent fork of touchHLE, a high-level emulator for iPhone OS applications.

## CPU backends

The bundled Dynarmic backend builds both ARM32 (`A32`) and ARM64 (`A64`) frontends by default. This restores compilation of the A64 frontend that was previously omitted from the default build.

This change only restores the A64 CPU frontend in the build. It does not yet make the complete iPhone OS guest runtime 64-bit: the Mach-O loader, guest pointer model, ABI, dyld, and framework shims still require a separate migration before 64-bit iPhone OS applications can run.

## Building

Build normally with Cargo. The Dynarmic submodule must be initialized:

```sh
git submodule update --init --recursive
cargo build
```
