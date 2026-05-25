/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Global C variables exported as non-lazy symbols by libSystem.
//!
//! Apple's libsystem exposes several `extern` globals that the dynamic
//! linker resolves at load time. Apps reach them either by `extern`
//! declaration or via convenience wrappers. The dyld linker in touchHLE
//! resolves "non-lazy" symbols using the `ConstantExports` mechanism;
//! each entry returns the *address* of a guest-side storage slot that
//! the runtime fills in with the appropriate value.
//!
//! Apple references:
//! * `environ(7)`:
//!   <https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man7/environ.7.html>
//! * `tzset(3)`:
//!   <https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/tzset.3.html>
//! * `<mach/vm_param.h>` for `vm_page_size` / `vm_page_mask`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::libc::time;
use crate::mem::{ConstPtr, ConstVoidPtr, Ptr};
use crate::Environment;

/// `extern char **environ;` — base of the program's environment vector.
/// touchHLE has no real environment block, so we expose a one-element
/// NUL-terminated array (`{ NULL }`) which is what newly-spawned
/// processes see when they `unsetenv` everything.
fn environ_ptr(env: &mut Environment) -> ConstVoidPtr {
    // `environ` itself is `char **`. The dyld layer wraps any
    // HostConstant value in an extra indirection, so what we return
    // here is *the address that will be written into the relocation
    // slot*. That address must point at a `char **`. We allocate a
    // one-pointer empty environment table and return a pointer to its
    // base address.
    let null_str: ConstPtr<u8> = Ptr::null();
    // Single-entry NUL-terminated env table.
    let table_ptr = env.mem.alloc_and_write(null_str);
    table_ptr.cast().cast_const()
}

/// `extern long timezone;` — seconds west of UTC for the current TZ.
/// On Apple platforms `tzset(3)` populates this; we ship UTC by
/// default (value 0). The address resolves to a `long` storage cell.
fn timezone_ptr(env: &mut Environment) -> ConstVoidPtr {
    time::get_timezone_ptr(env).cast().cast_const()
}

/// `extern int daylight;` — boolean flag set by `tzset(3)` when DST is
/// observed in the current TZ. UTC has no DST → 0.
fn daylight_ptr(env: &mut Environment) -> ConstVoidPtr {
    time::get_daylight_ptr(env).cast().cast_const()
}

/// `extern char *tzname[2];` — names of standard and DST time zones.
fn tzname_ptr(env: &mut Environment) -> ConstVoidPtr {
    time::get_tzname_array_ptr(env).cast().cast_const()
}

/// `extern vm_size_t vm_page_size;` — 4 KiB on all 32-bit iOS devices.
fn vm_page_size_ptr(env: &mut Environment) -> ConstVoidPtr {
    let value: u32 = crate::mem::PAGE_SIZE;
    env.mem.alloc_and_write(value).cast().cast_const()
}

/// `extern vm_size_t vm_page_mask;` — `vm_page_size - 1` (page offset
/// mask). See `<mach/vm_param.h>`.
fn vm_page_mask_ptr(env: &mut Environment) -> ConstVoidPtr {
    let value: u32 = crate::mem::PAGE_SIZE - 1;
    env.mem.alloc_and_write(value).cast().cast_const()
}

/// `extern vm_size_t vm_page_shift;` — log2 of `vm_page_size`. 12 for
/// 4 KiB pages.
fn vm_page_shift_ptr(env: &mut Environment) -> ConstVoidPtr {
    let value: u32 = crate::mem::PAGE_SIZE.trailing_zeros();
    env.mem.alloc_and_write(value).cast().cast_const()
}

pub const CONSTANTS: ConstantExports = &[
    ("_environ", HostConstant::Custom(environ_ptr)),
    ("_timezone", HostConstant::Custom(timezone_ptr)),
    ("_daylight", HostConstant::Custom(daylight_ptr)),
    ("_tzname", HostConstant::Custom(tzname_ptr)),
    ("_vm_page_size", HostConstant::Custom(vm_page_size_ptr)),
    ("_vm_page_mask", HostConstant::Custom(vm_page_mask_ptr)),
    ("_vm_page_shift", HostConstant::Custom(vm_page_shift_ptr)),
];
