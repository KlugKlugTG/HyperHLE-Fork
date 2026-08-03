# ARM64 implementation status

## Current state

The emulator currently executes ARMv7/A32 and stores guest pointers and ABI values in 32-bit types. Dynarmic already contains an A64 frontend, but enabling that frontend alone does not provide ARM64 support.

## Required migration

1. Add a guest architecture mode selected from the Mach-O slice.
2. Add an A64 CPU wrapper with 64-bit registers, PSTATE, FPCR/FPSR, 128-bit SIMD registers, A64 memory callbacks, SVC/BRK handling, and exclusive accesses.
3. Add a 64-bit virtual-memory backend. The existing `Mem` reserves exactly 4 GiB and cannot represent 64-bit pointers.
4. Split ABI and pointer-bearing framework APIs into A32 and A64 paths. A64 uses x0-x7 for arguments and x0 for returns, 8-byte pointers, and 16-byte stack alignment.
5. Extend Mach-O loading for `CPU_TYPE_ARM64`, `LC_SEGMENT_64`, 64-bit entry state, 64-bit symbol/relocation addresses, chained/fixup binding as used by iOS 11-era binaries, and ARM64 relocations.
6. Add an A64 dyld/libc runtime: pthread/TLS, errno, malloc, Objective-C messaging, exception/unwind support, system calls, and framework shims with 64-bit structures.
7. Add tests using real ARM64 Mach-O fixtures and run Android builds after each vertical slice.

## Compatibility rule

The ARMv7 path must remain unchanged. New 64-bit code should be selected only for an ARM64 Mach-O slice; existing ARM/Thumb binaries continue through the current CPU, memory, ABI, and dyld paths.

## Honest limitation

This is a multi-subsystem port, not a one-file toggle. Until the CPU, memory, ABI, loader, and runtime layers are implemented together, an iOS 11+ ARM64 application cannot run reliably.
