/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use crate::dyld::FunctionExports;
use crate::environment::Environment;
use crate::export_c_func;
use crate::libc::errno::{set_errno, EINVAL, EIO};
use crate::libc::posix_io;
use crate::libc::posix_io::{off_t, open_direct, FileDescriptor, SEEK_SET};
use crate::mem::{ConstPtr, GuestUSize, MutVoidPtr, PAGE_SIZE_ALIGN_MASK};
use std::collections::HashMap;

#[allow(dead_code)]
const MAP_FILE: i32 = 0x0000;
const MAP_ANON: i32 = 0x1000;

#[derive(Default)]
pub struct State {
    /// Keeping track of `mmap` allocations
    allocations: HashMap<MutVoidPtr, GuestUSize>,
}

/// For files, our implementation of mmap is really simple:
/// it's just load entirety of file in memory!
fn mmap(
    env: &mut Environment,
    addr: MutVoidPtr,
    len: GuestUSize,
    prot: i32,
    flags: i32,
    fd: FileDescriptor,
    offset: off_t,
) -> MutVoidPtr {
    // TODO: handle errno properly
    set_errno(env, 0);
    log_dbg!(
        "mmap({:?}, {}, {}, {}, {}, {})",
        addr,
        len,
        prot,
        flags,
        fd,
        offset
    );

    // TODO: use vm_allocate() instead
    let ptr = env.mem.calloc(len);

    if (flags & MAP_ANON) != 0 {
        assert!(ptr.to_bits() & PAGE_SIZE_ALIGN_MASK == 0);

        // Убираем жесткие assert_eq!(fd, -1) и assert_eq!(offset, 0).
        // В реальной iOS/Darwin при наличии флага MAP_ANON аргументы fd и
        // offset
        // просто игнорируются ОС. Движки вроде Adobe AIR передают сюда мусор.
        if fd != -1 || offset != 0 {
            log_dbg!("Warning: mmap MAP_ANON called with fd={} and offset={}. Ignoring them as per OS behavior.", fd, offset);
        }

        if !addr.is_null() {
            // POSIX `mmap` documents the `addr` argument as a hint that
            // implementations may ignore. We always allocate from the
            // guest heap, so the actual placement is the heap allocator's
            // choice. Apps that genuinely require fixed-address mappings
            // would set MAP_FIXED, which we'd then need to honour
            // separately. Demoted to debug to keep Mono/Unity startup
            // logs readable; the host-vs-hint mismatch is not an error.
            log_dbg!(
                "mmap MAP_ANON ignoring hint for address {:?}, actual is {:?}",
                addr,
                ptr
            );
        }
    } else {
        // File-backed mmap: read file content into the allocated buffer.
        if !addr.is_null() {
            log_dbg!(
                "mmap file-backed ignoring hint for address {:?}, actual is {:?}",
                addr,
                ptr
            );
        }
        // Seek to the requested offset. If the seek fails (e.g. bad fd),
        // return MAP_FAILED (-1 as pointer) instead of crashing.
        let new_offset = posix_io::lseek(env, fd, offset, SEEK_SET);
        if new_offset != offset {
            log!(
                "Warning: mmap: lseek to offset {} failed (returned {}); returning MAP_FAILED",
                offset,
                new_offset
            );
            env.mem.free(ptr);
            set_errno(env, EIO);
            return MutVoidPtr::from_bits(0xFFFFFFFF); // MAP_FAILED
        }

        let read = posix_io::read(env, fd, ptr, len);
        if (read as u32) < len {
            log!(
                "Warning: mmap: read only {} of {} bytes from fd {}; padding remainder with zeros",
                read,
                len,
                fd
            );
            // Remainder is already zeroed (calloc)
        }
    }

    assert!(!env.libc_state.mmap.allocations.contains_key(&ptr));
    env.libc_state.mmap.allocations.insert(ptr, len);

    ptr
}

fn munmap(env: &mut Environment, addr: MutVoidPtr, len: GuestUSize) -> i32 {
    // TODO: handle errno properly
    set_errno(env, 0);
    log_dbg!("munmap({:?}, {})", addr, len);

    if len == 0 {
        // Darwin returns EINVAL for `munmap(addr, 0)`, but several apps
        // (e.g. the UE3-based BioShock port) call it this way in a loop to
        // release cached buffers and treat any failure as fatal for their
        // allocator bookkeeping. Be permissive: if the whole mapping at
        // `addr` is known, release it in full (its recorded length is what
        // the caller means); otherwise treat the call as a successful no-op.
        // This matches Linux's munmap(NULL-adjacent) tolerance and keeps the
        // guest allocator's state consistent.
        if let Some(&expected_len) = env.libc_state.mmap.allocations.get(&addr) {
            log_dbg!(
                "munmap({:?}, 0): zero length, unmapping whole {}-byte mapping",
                addr,
                expected_len
            );
            env.mem.free(addr);
            env.libc_state.mmap.allocations.remove(&addr);
        } else {
            log_dbg!("munmap({:?}, 0): unknown mapping, treating as no-op", addr);
        }
        set_errno(env, 0);
        return 0;
    }

    if let Some(&expected_len) = env.libc_state.mmap.allocations.get(&addr) {
        if expected_len != len {
            log_dbg!(
                "munmap({:?}, {}): length mismatch (expected {}), proceeding anyway",
                addr,
                len,
                expected_len
            );
        }
        env.mem.free(addr);
        env.libc_state.mmap.allocations.remove(&addr);
        0 // success
    } else {
        // BioShock's engine probes munmap() on addresses it did not mmap
        // (likely ASLR-guessing from a forked helper) and ignores the result.
        // Keep the POSIX-correct -1/EINVAL but log at debug level so the log
        // stays readable.
        log_dbg!(
            "munmap({:?}, {}): unknown mapping, returning -1",
            addr,
            len
        );
        set_errno(env, EINVAL);
        -1
    }
}

// Darwin `madvise` advice values (sys/mman.h).
const MADV_NORMAL: i32 = 0;
const MADV_RANDOM: i32 = 1;
const MADV_SEQUENTIAL: i32 = 2;
const MADV_WILLNEED: i32 = 3;
const MADV_DONTNEED: i32 = 4;
const MADV_FREE: i32 = 5;
const MADV_ZERO_WIRED_PAGES: i32 = 6;
const MADV_FREE_REUSABLE: i32 = 7;
const MADV_FREE_REUSE: i32 = 8;
const MADV_CAN_REUSE: i32 = 9;

/// Guest memory is a plain host allocation with no paging, so every advice
/// is a hint we can safely ignore. Only the argument validation that a real
/// kernel performs is emulated: unknown advice values and unaligned
/// addresses fail with `EINVAL`.
fn madvise(env: &mut Environment, addr: MutVoidPtr, len: GuestUSize, advice: i32) -> i32 {
    log_dbg!("madvise({:?}, {}, {})", addr, len, advice);
    match advice {
        MADV_NORMAL
        | MADV_RANDOM
        | MADV_SEQUENTIAL
        | MADV_WILLNEED
        | MADV_DONTNEED
        | MADV_FREE
        | MADV_ZERO_WIRED_PAGES
        | MADV_FREE_REUSABLE
        | MADV_FREE_REUSE
        | MADV_CAN_REUSE => {}
        _ => {
            set_errno(env, EINVAL);
            return -1;
        }
    }
    if addr.to_bits() & PAGE_SIZE_ALIGN_MASK != 0 {
        set_errno(env, EINVAL);
        return -1;
    }
    set_errno(env, 0);
    0
}

fn shm_open(env: &mut Environment, name: ConstPtr<u8>, oflag: i32, mode: u32) -> i32 {
    set_errno(env, 0);

    let name_str = env.mem.cstr_at_utf8(name).unwrap_or("<invalid>");
    log_dbg!("shm_open({:?}, {:#x}, {:#x})", name_str, oflag, mode);

    // Используем open_direct! Параметр mode для эмулятора здесь не нужен,
    // поэтому просто передаем env, name и oflag.
    open_direct(env, name, oflag)
}

/// `int shm_unlink(const char *name)`
///
/// POSIX shared-memory unlink: removes the named shared-memory object.
/// Regions opened via `shm_open` are backed by our fs layer, where `unlink`
/// already implements remove-on-last-close semantics. Missing regions return
/// -1/ENOENT per POSIX.
fn shm_unlink(env: &mut Environment, name: ConstPtr<u8>) -> i32 {
    crate::libc::unistd::unlink(env, name)
}

fn mprotect(env: &mut Environment, addr: MutVoidPtr, len: GuestUSize, prot: i32) -> i32 {
    // POSIX `int mprotect(void *addr, size_t len, int prot)`: returns 0
    // on success, -1 on failure with errno set.
    //
    // touchHLE doesn't enforce per-page memory protections — the entire
    // guest address space is treated as RW (and code pages as RX through
    // the JIT). However, returning -1 + ENOTSUP for every mprotect call
    // is wrong: it makes Mono/Boehm GC and Unity's runtime think the
    // protection change failed during JIT/GC initialization, which can
    // leave them in a broken state. Real Darwin kernels never fail
    // mprotect for the address ranges that mmap'd allocations sit in,
    // so the correct behavior is "succeed silently as a no-op".
    //
    // Reference: POSIX mprotect(2) — return value is 0 on success, -1 on
    // error; the only documented errors apply to invalid/non-mapped
    // address ranges, which our guest mmap allocator handles by always
    // returning a valid range.
    log_dbg!(
        "mprotect({:?}, {}, {:#x}) -> 0 (no-op; touchHLE does not enforce per-page protections)",
        addr,
        len,
        prot
    );
    set_errno(env, 0);
    0
}

/// `int mlock(const void *addr, size_t len)` — lock a region of memory
/// so it stays resident in physical RAM. touchHLE keeps the entire
/// guest address space resident in the host process at all times, so
/// there is nothing to pin: the memory is already non-pageable from the
/// guest's perspective. Real Darwin returns 0 on success, so we report
/// success as a no-op (returning -1 here would make guest code that
/// relies on mlock — e.g. crypto/keychain libraries protecting secrets,
/// or audio engines pinning buffers — treat initialization as failed).
///
/// Reference: POSIX/Darwin mlock(2) — returns 0 on success, -1 with
/// errno on failure.
fn mlock(env: &mut Environment, addr: ConstPtr<u8>, len: GuestUSize) -> i32 {
    log_dbg!(
        "mlock({:?}, {}) -> 0 (no-op; guest memory is always resident)",
        addr,
        len
    );
    set_errno(env, 0);
    0
}

/// `int munlock(const void *addr, size_t len)` — unlock a region
/// previously locked with `mlock`. Mirrors [mlock]: a no-op that
/// succeeds.
///
/// Reference: POSIX/Darwin munlock(2) — returns 0 on success.
fn munlock(env: &mut Environment, addr: ConstPtr<u8>, len: GuestUSize) -> i32 {
    log_dbg!(
        "munlock({:?}, {}) -> 0 (no-op; guest memory is always resident)",
        addr,
        len
    );
    set_errno(env, 0);
    0
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(mmap(_, _, _, _, _, _)),
    export_c_func!(munmap(_, _)),
    export_c_func!(madvise(_, _, _)),
    export_c_func!(shm_open(_, _, _)),
    export_c_func!(shm_unlink(_)),
    export_c_func!(mprotect(_, _, _)),
    export_c_func!(mlock(_, _)),
    export_c_func!(munlock(_, _)),
];
