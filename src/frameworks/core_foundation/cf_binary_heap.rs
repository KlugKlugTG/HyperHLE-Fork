/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFBinaryHeap`.
//!
//! A binary heap (priority queue) from Core Foundation.
//!
//! Apple references:
//! - [CFBinaryHeap Reference](https://developer.apple.com/documentation/corefoundation/cfbinaryheap)
//! - [CFBinaryHeapCallBacks](https://developer.apple.com/documentation/corefoundation/cfbinaryheapcallbacks)
//! - [CFBinaryHeapCompareContext](https://developer.apple.com/documentation/corefoundation/cfbinaryheapcomparecontext)

use super::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use super::{CFIndex, CFTypeRef};
use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, Ptr, SafeRead};
use crate::objc::{id, nil, release, retain};
use crate::Environment;

pub type CFBinaryHeapRef = CFTypeRef;

/// Apple `CFBinaryHeapCallBacks` structure.
/// Version 0 layout (5 function pointers).
#[repr(C, packed)]
#[allow(dead_code)]
pub struct CFBinaryHeapCallBacks {
    pub version: CFIndex,
    pub retain: ConstVoidPtr,
    pub release: ConstVoidPtr,
    pub copy_description: ConstVoidPtr,
    pub compare: ConstVoidPtr,
}
unsafe impl SafeRead for CFBinaryHeapCallBacks {}

/// Apple `CFBinaryHeapCompareContext` structure.
#[repr(C, packed)]
#[allow(dead_code)]
pub struct CFBinaryHeapCompareContext {
    pub version: CFIndex,
    pub info: ConstVoidPtr,
    pub retain: ConstVoidPtr,
    pub release: ConstVoidPtr,
    pub copy_description: ConstVoidPtr,
}
unsafe impl SafeRead for CFBinaryHeapCompareContext {}

struct CFBinaryHeapHostObject {
    values: Vec<ConstVoidPtr>,
    callbacks: CFBinaryHeapCallBacks,
    compare_context: CFBinaryHeapCompareContext,
}
impl crate::objc::HostObject for CFBinaryHeapHostObject {}

fn CFBinaryHeapCreate(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    _capacity: CFIndex,
    callbacks: ConstPtr<CFBinaryHeapCallBacks>,
    compare_context: ConstPtr<CFBinaryHeapCompareContext>,
) -> CFBinaryHeapRef {
    if !allocator.is_null() && allocator != kCFAllocatorDefault {
        log_dbg!("CFBinaryHeapCreate: ignoring custom allocator {:?}", allocator);
    }

    let callbacks = if callbacks.is_null() {
        CFBinaryHeapCallBacks {
            version: 0,
            retain: Ptr::null(),
            release: Ptr::null(),
            copy_description: Ptr::null(),
            compare: Ptr::null(),
        }
    } else {
        env.mem.read(callbacks)
    };

    let compare_context = if compare_context.is_null() {
        CFBinaryHeapCompareContext {
            version: 0,
            info: Ptr::null(),
            retain: Ptr::null(),
            release: Ptr::null(),
            copy_description: Ptr::null(),
        }
    } else {
        env.mem.read(compare_context)
    };

    let host_object = Box::new(CFBinaryHeapHostObject {
        values: Vec::new(),
        callbacks,
        compare_context,
    });

    let class = env.objc.get_known_class("NSObject", &mut env.mem);
    env.objc.alloc_object(class, host_object, &mut env.mem)
}

fn CFBinaryHeapAddValue(env: &mut Environment, heap: CFBinaryHeapRef, value: ConstVoidPtr) {
    if heap == nil {
        log!("Warning: CFBinaryHeapAddValue called with NULL heap");
        return;
    }
    if value.is_null() {
        log!("Warning: CFBinaryHeapAddValue called with NULL value");
        return;
    }

    let host_obj = env.objc.borrow_mut::<CFBinaryHeapHostObject>(heap);

    // Call retain callback if provided and store the retained pointer.
    let stored = if !host_obj.callbacks.retain.is_null() {
        let retain_fn = GuestFunction::from_addr_with_thumb_bit(host_obj.callbacks.retain.to_bits());
        retain_fn.call_from_host(env, (value,))
    } else if !value.is_null() {
        // Default CF-type retain behaviour: treat as NSObject and retain.
        let value_id: id = value.cast().cast_mut();
        retain(env, value_id);
        value
    } else {
        value
    };

    host_obj.values.push(stored);
}

/// Helper: call the guest compare callback or fall back to pointer comparison.
fn compare_values(
    env: &mut Environment,
    compare_ptr: ConstVoidPtr,
    context_info: ConstVoidPtr,
    a: ConstVoidPtr,
    b: ConstVoidPtr,
) -> CFIndex {
    if !compare_ptr.is_null() {
        let compare_fn = GuestFunction::from_addr_with_thumb_bit(compare_ptr.to_bits());
        let result: CFIndex = compare_fn.call_from_host(env, (a, b, context_info));
        return result;
    }
    // Default: pointer comparison (arbitrary but stable).
    a.to_bits().cmp(&b.to_bits()) as CFIndex
}

fn CFBinaryHeapGetMinimum(env: &mut Environment, heap: CFBinaryHeapRef) -> ConstVoidPtr {
    if heap == nil {
        return Ptr::null();
    }
    let host_obj = env.objc.borrow::<CFBinaryHeapHostObject>(heap);
    if host_obj.values.is_empty() {
        return Ptr::null();
    }

    let compare_ptr = host_obj.callbacks.compare;
    let context_info = host_obj.compare_context.info;
    let mut min_idx = 0;
    let mut min_val = host_obj.values[0];
    for (i, &val) in host_obj.values.iter().enumerate().skip(1) {
        if compare_values(env, compare_ptr, context_info, val, min_val) < 0 {
            min_idx = i;
            min_val = val;
        }
    }
    host_obj.values[min_idx]
}

fn CFBinaryHeapRemoveMinimumValue(env: &mut Environment, heap: CFBinaryHeapRef) {
    if heap == nil {
        return;
    }
    let host_obj = env.objc.borrow_mut::<CFBinaryHeapHostObject>(heap);
    if host_obj.values.is_empty() {
        return;
    }

    let compare_ptr = host_obj.callbacks.compare;
    let context_info = host_obj.compare_context.info;
    let mut min_idx = 0;
    let mut min_val = host_obj.values[0];
    for (i, &val) in host_obj.values.iter().enumerate().skip(1) {
        if compare_values(env, compare_ptr, context_info, val, min_val) < 0 {
            min_idx = i;
            min_val = val;
        }
    }

    let removed = host_obj.values.remove(min_idx);

    // Call release callback if provided.
    if !host_obj.callbacks.release.is_null() {
        let release_fn =
            GuestFunction::from_addr_with_thumb_bit(host_obj.callbacks.release.to_bits());
        let _ = release_fn.call_from_host(env, (removed,));
    } else if !removed.is_null() {
        let removed_id: id = removed.cast().cast_mut();
        release(env, removed_id);
    }
}

fn CFBinaryHeapGetCount(env: &mut Environment, heap: CFBinaryHeapRef) -> CFIndex {
    if heap == nil {
        return 0;
    }
    env.objc.borrow::<CFBinaryHeapHostObject>(heap).values.len() as CFIndex
}

fn CFBinaryHeapContainsValue(
    env: &mut Environment,
    heap: CFBinaryHeapRef,
    value: ConstVoidPtr,
) -> bool {
    if heap == nil {
        return false;
    }
    let host_obj = env.objc.borrow::<CFBinaryHeapHostObject>(heap);
    host_obj.values.iter().any(|&v| v == value)
}

fn CFBinaryHeapRemoveAllValues(env: &mut Environment, heap: CFBinaryHeapRef) {
    if heap == nil {
        return;
    }
    let host_obj = env.objc.borrow_mut::<CFBinaryHeapHostObject>(heap);
    for &value in &host_obj.values {
        if !host_obj.callbacks.release.is_null() {
            let release_fn =
                GuestFunction::from_addr_with_thumb_bit(host_obj.callbacks.release.to_bits());
            let _ = release_fn.call_from_host(env, (value,));
        } else if !value.is_null() {
            let value_id: id = value.cast().cast_mut();
            release(env, value_id);
        }
    }
    host_obj.values.clear();
}

fn CFBinaryHeapGetValues(
    env: &mut Environment,
    heap: CFBinaryHeapRef,
    values: MutPtr<ConstVoidPtr>,
) {
    if heap == nil {
        return;
    }
    let host_obj = env.objc.borrow::<CFBinaryHeapHostObject>(heap);
    if values.is_null() {
        return;
    }
    for (i, &val) in host_obj.values.iter().enumerate() {
        env.mem.write(values + i as u32, val);
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFBinaryHeapCreate(_, _, _, _)),
    export_c_func!(CFBinaryHeapAddValue(_, _)),
    export_c_func!(CFBinaryHeapGetMinimum(_)),
    export_c_func!(CFBinaryHeapRemoveMinimumValue(_)),
    export_c_func!(CFBinaryHeapGetCount(_)),
    export_c_func!(CFBinaryHeapContainsValue(_, _)),
    export_c_func!(CFBinaryHeapRemoveAllValues(_)),
    export_c_func!(CFBinaryHeapGetValues(_, _)),
];
