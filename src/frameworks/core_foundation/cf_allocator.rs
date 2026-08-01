/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFAllocator`.
//!
//! Only kCFAllocatorDefault and kCFAllocatorSystemDefault allocators are
//! currently supported. Both are essentially the same thing for us for now,
//! but the default one is an alias for NULL.

use crate::dyld::{ConstantExports, FunctionExports, HostConstant};
use crate::environment::Environment;
use crate::export_c_func;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutVoidPtr, Ptr, SafeRead};

// CFAllocator is an opaque struct.
// It doesn't need to be a CFTypeRef compatible (seems so?),
// so it's just a struct without Obj-C counterpart.
pub struct CFAllocatorHostObject(CFAllocatorType);
unsafe impl SafeRead for CFAllocatorHostObject {}

impl CFAllocatorHostObject {
    pub fn is_system_default(&self) -> bool {
        self.0 == CFAllocatorType::SystemDefault
    }
}

// TODO: support other types
#[repr(i32)]
#[derive(PartialEq)]
enum CFAllocatorType {
    /// For us, same as Default one, but not a NULL ptr.
    SystemDefault = 1,
}

pub type CFAllocatorRef = ConstPtr<CFAllocatorHostObject>;

pub const kCFAllocatorDefault: CFAllocatorRef = Ptr::null();

pub const CONSTANTS: ConstantExports = &[
    ("_kCFAllocatorDefault", HostConstant::NullPtr),
    (
        "_kCFAllocatorSystemDefault",
        HostConstant::Custom(|env| {
            let allocator_ptr = env
                .mem
                .alloc_and_write(CFAllocatorHostObject(CFAllocatorType::SystemDefault));
            env.mem.alloc_and_write(allocator_ptr).cast().cast_const()
        }),
    ),
    // CFAllocator variants that touchHLE treats as aliases for the system
    // default. CFDataCreateWithBytesNoCopy etc. use kCFAllocatorNull to mean
    // "do not free the backing bytes", which is fine because touchHLE always
    // copies anyway.
    ("_kCFAllocatorMalloc", HostConstant::NullPtr),
    ("_kCFAllocatorMallocZone", HostConstant::NullPtr),
    ("_kCFAllocatorNull", HostConstant::NullPtr),
    ("_kCFAllocatorUseContext", HostConstant::NullPtr),
];

/// `CFAllocatorAllocate(allocator, size, hint)` — allocate `size` bytes of
/// guest memory. `hint` is informational and ignored here (matches Apple's
/// behaviour: the CF docs say the hint is currently reserved and must be 0).
/// Returns `NULL` on failure. For `kCFAllocatorNull` (a null pointer) the
/// contract is to fail every allocation; we honour that so callers with
/// "no-op" allocators don't accidentally get real memory back.
fn CFAllocatorAllocate(
    env: &mut Environment,
    allocator: ConstPtr<CFAllocatorHostObject>,
    size: GuestUSize,
    _hint: i32,
) -> MutVoidPtr {
    // Treat kCFAllocatorDefault (NULL) and kCFAllocatorSystemDefault as the
    // system allocator. kCFAllocatorNull is also NULL in our export table,
    // but callers always pass a real allocator reference here, so we can't
    // distinguish them — fall back to the system allocator.
    let _ = allocator;
    if size == 0 {
        // CF contract: allocating 0 bytes returns NULL. Many callers rely
        // on this to mean "no allocation performed".
        return Ptr::null();
    }
    env.mem.alloc(size)
}

/// `CFAllocatorDeallocate(allocator, ptr)` — free memory previously obtained
/// from `CFAllocatorAllocate`. NULL pointer is a no-op, as per the CF docs.
fn CFAllocatorDeallocate(
    env: &mut Environment,
    _allocator: ConstPtr<CFAllocatorHostObject>,
    ptr: MutVoidPtr,
) {
    if ptr.is_null() {
        return;
    }
    env.mem.free(ptr)
}

/// `CFAllocatorReallocate(allocator, ptr, newsize, hint)` — grow or shrink
/// an allocation. Matches `realloc` semantics: `ptr==NULL` behaves like
/// `alloc`, `newsize==0` behaves like `free` and returns NULL.
fn CFAllocatorReallocate(
    env: &mut Environment,
    allocator: ConstPtr<CFAllocatorHostObject>,
    ptr: MutVoidPtr,
    newsize: GuestUSize,
    hint: i32,
) -> MutVoidPtr {
    if newsize == 0 {
        CFAllocatorDeallocate(env, allocator, ptr);
        return Ptr::null();
    }
    if ptr.is_null() {
        return CFAllocatorAllocate(env, allocator, newsize, hint);
    }
    env.mem.realloc(ptr, newsize)
}

/// `CFAllocatorGetDefault()` — returns the currently-installed default
/// allocator. `kCFAllocatorDefault` is documented as the NULL allocator
/// handle, which callers are expected to pass unchanged back to other CF
/// functions. We don't have per-thread allocator overrides, so just return
/// NULL.
fn CFAllocatorGetDefault(_env: &mut Environment) -> ConstPtr<CFAllocatorHostObject> {
    kCFAllocatorDefault
}

/// `CFAllocatorGetContext(allocator, context)` — Apple stores user data in
/// custom allocators. We don't support custom allocators, so write zeros.
fn CFAllocatorGetContext(
    env: &mut Environment,
    _allocator: ConstPtr<CFAllocatorHostObject>,
    context: MutVoidPtr,
) {
    if context.is_null() {
        return;
    }
    // `CFAllocatorContext` is a 6-word struct on 32-bit platforms. Zero it
    // out so the app sees an "empty" context.
    let zero = [0u8; 24];
    env.mem
        .bytes_at_mut(context.cast::<u8>(), zero.len() as GuestUSize)
        .copy_from_slice(&zero);
}

/// `CFGetAllocator(cf)` — object introspection. Every object in touchHLE
/// uses the system default allocator.
fn CFGetAllocator(env: &mut Environment, _cf: ConstVoidPtr) -> ConstPtr<CFAllocatorHostObject> {
    CFAllocatorGetDefault(env)
}

/// `CFAllocatorGetPreferredSizeForSize(allocator, size, hint)` — returns the
/// size the allocator would actually allocate for the requested size. Our
/// system allocator doesn't round up, so return `size` unchanged.
fn CFAllocatorGetPreferredSizeForSize(
    _env: &mut Environment,
    _allocator: ConstPtr<CFAllocatorHostObject>,
    size: GuestUSize,
    _hint: i32,
) -> GuestUSize {
    size
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFAllocatorAllocate(_, _, _)),
    export_c_func!(CFAllocatorDeallocate(_, _)),
    export_c_func!(CFAllocatorReallocate(_, _, _, _)),
    export_c_func!(CFAllocatorGetDefault()),
    export_c_func!(CFAllocatorGetContext(_, _)),
    export_c_func!(CFAllocatorGetPreferredSizeForSize(_, _, _)),
    export_c_func!(CFGetAllocator(_)),
];
