/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `libBlocksRuntime` — Apple Blocks ABI helpers.
//!
//! These functions are called by the compiler-generated copy/dispose
//! helpers when an Objective-C block captures a `__strong` ObjC object,
//! a `__weak` reference, another block, or a `__block` storage variable.
//! They are documented in the
//! [Blocks ABI](https://clang.llvm.org/docs/Block-ABI-Apple.html#imported-variables-1).
//!
//! Stack blocks and byref captures are promoted to guest heap storage before
//! asynchronous use. Compiler-generated copy/dispose helpers own captures.

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstPtr, ConstVoidPtr, MutVoidPtr, Ptr};
use crate::objc::{id, release, retain};
use crate::Environment;
use std::sync::atomic::{AtomicU32, Ordering};

/// Bit-flag values passed to `_Block_object_assign` / `_Block_object_dispose`.
/// See the Blocks ABI document referenced above.
const BLOCK_FIELD_IS_OBJECT: i32 = 3;
const BLOCK_FIELD_IS_BLOCK: i32 = 7;
const BLOCK_FIELD_IS_BYREF: i32 = 8;
const BLOCK_FIELD_IS_WEAK: i32 = 16;
const BLOCK_BYREF_CALLER: i32 = 128;

const BLOCK_NEEDS_FREE: u32 = 1 << 24;
const BLOCK_HAS_COPY_DISPOSE: u32 = 1 << 25;
const BLOCK_IS_GLOBAL: u32 = 1 << 28;
const REFCOUNT_MASK: u32 = 0xffff;
const BLOCK_HAS_SIGNATURE: u32 = 1 << 30;
const BLOCK_HAS_EXTENDED_LAYOUT: u32 = 1 << 31;
const BLOCK_LAYOUT_FLAGS: u32 = BLOCK_NEEDS_FREE
    | BLOCK_HAS_COPY_DISPOSE
    | BLOCK_IS_GLOBAL
    | BLOCK_HAS_SIGNATURE
    | BLOCK_HAS_EXTENDED_LAYOUT;
const MAX_BLOCK_SIZE: u32 = 64 * 1024 * 1024;

/// How many distinct Blocks runtime class descriptors we remember. dyld
/// allocates one per symbol name (`_NSConcreteStackBlock`,
/// `_NSConcreteGlobalBlock`), so a handful of slots is plenty.
const BLOCK_CLASS_DESCRIPTOR_SLOTS: usize = 8;
static BLOCK_CLASS_DESCRIPTORS: [AtomicU32; BLOCK_CLASS_DESCRIPTOR_SLOTS] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

/// Record the guest address of a Blocks runtime class descriptor allocated by
/// dyld (`_NSConcreteStackBlock` / `_NSConcreteGlobalBlock`). Every block
/// literal's `isa` is relocated to one of these, so the `isa` is the only
/// reliable way to tell a block apart from an ordinary object.
///
/// Called by `dyld` when it patches such a relocation; idempotent, and
/// addresses beyond the slot count are simply not remembered (the layout
/// fallback in [`is_block_object`] still applies to them).
pub fn register_block_class_descriptor(addr: u32) {
    if addr == 0 {
        return;
    }
    for slot in &BLOCK_CLASS_DESCRIPTORS {
        match slot.compare_exchange(0, addr, Ordering::Relaxed, Ordering::Relaxed) {
            // Claimed a free slot, or this address is already remembered.
            Ok(_) => return,
            Err(current) if current == addr => return,
            Err(_) => continue,
        }
    }
}

fn isa_is_block_class_descriptor(isa: u32) -> bool {
    BLOCK_CLASS_DESCRIPTORS
        .iter()
        .any(|slot| slot.load(Ordering::Relaxed) == isa)
}

fn valid_block_layout(
    isa: u32,
    flags: u32,
    invoke: u32,
    descriptor: u32,
    descriptor_size: u32,
) -> bool {
    isa != 0
        && flags & BLOCK_LAYOUT_FLAGS != 0
        && invoke & !1 != 0
        && descriptor != 0
        && (20..=MAX_BLOCK_SIZE).contains(&descriptor_size)
}

/// Is `block` an Objective-C block literal?
///
/// A block is identified by its `isa`, which dyld relocates to one of the
/// Blocks runtime class descriptors registered with
/// [`register_block_class_descriptor`].
///
/// The `isa` check matters because guessing from the object layout is not
/// safe. The old heuristic — non-null `isa`, *some* high bit set in the second
/// word, a non-null fourth word and a "plausible" descriptor pointer — also
/// matches plenty of ordinary Objective-C objects: any object whose first ivar
/// holds a value with bits 24–28/30/31 set (a heap pointer above 128 MiB, a
/// negative float, a colour, a bitfield, …) and whose fourth word points at
/// something whose second word happens to be a small number. When such an
/// object is misidentified, `objc_msgSend` routes its `retain`, `release`,
/// `copy`, `copyWithZone:` and `autorelease` to `_Block_copy`/`_Block_release`
/// instead of the class's own implementation: `release` decrements an arbitrary
/// ivar, then calls whatever pointer sits at `descriptor + 12` and `free()`s
/// the object, while `copy`/`retain` hand back a freshly allocated alias of it.
/// The guest then runs on with a corrupted object and a bogus function pointer
/// — the "UndefinedInstruction spam / the game is broken" symptom.
///
/// If the descriptor addresses are unknown (an app that never links the Blocks
/// runtime through a path dyld registers, e.g. one that ships its own static
/// copy of libBlocksRuntime), fall back to the ABI layout check, but only for a
/// receiver whose `isa` is *not* a registered class: every real object's `isa`
/// resolves to a class in the runtime, so this rejects the ordinary objects the
/// heuristic used to misfire on.
pub fn is_block_object(env: &Environment, block: ConstVoidPtr) -> bool {
    if block.is_null() {
        return false;
    }
    let words = block.cast::<u32>();
    let isa: u32 = env.mem.read(words);
    if isa == 0 {
        return false;
    }
    if isa_is_block_class_descriptor(isa) {
        return true;
    }
    // Unknown `isa`: a real object's class is always registered with the
    // runtime, so only an unregistered `isa` can still be a block.
    if env.objc.get_host_object(id::from_bits(isa)).is_some() {
        return false;
    }
    let flags: u32 = env.mem.read(words + 1);
    let invoke: u32 = env.mem.read(words + 3);
    let descriptor_addr: u32 = env.mem.read(words + 4);
    if descriptor_addr == 0 {
        return false;
    }
    let descriptor = ConstPtr::<u32>::from_bits(descriptor_addr);
    let descriptor_size: u32 = env.mem.read(descriptor + 1);
    valid_block_layout(isa, flags, invoke, descriptor_addr, descriptor_size)
}

fn add_reference(env: &mut Environment, flags_ptr: crate::mem::MutPtr<u32>) {
    let flags: u32 = env.mem.read(flags_ptr);
    // A saturated reference count is immortal, rather than wrapping to zero.
    if flags & REFCOUNT_MASK != REFCOUNT_MASK {
        env.mem.write(flags_ptr, flags + 1);
    }
}

fn remove_reference(env: &mut Environment, flags_ptr: crate::mem::MutPtr<u32>) -> bool {
    let flags: u32 = env.mem.read(flags_ptr);
    let count = flags & REFCOUNT_MASK;
    if count == REFCOUNT_MASK {
        return false;
    }
    assert!(count != 0, "Releasing a block with zero references");
    env.mem.write(flags_ptr, flags - 1);
    count == 1
}

/// Promote a stack block, or retain an existing heap block. Global blocks
/// are immortal. All offsets below are words in the 32-bit Apple Blocks ABI.
pub fn _Block_copy(env: &mut Environment, block: ConstVoidPtr) -> ConstVoidPtr {
    if block.is_null() {
        return block;
    }
    let words = block.cast::<u32>();
    let flags: u32 = env.mem.read(words + 1);
    if flags & BLOCK_IS_GLOBAL != 0 {
        return block;
    }
    if flags & BLOCK_NEEDS_FREE != 0 {
        add_reference(env, (words + 1).cast_mut());
        return block;
    }
    let descriptor: crate::mem::ConstPtr<u32> = env.mem.read((words + 4).cast());
    let size: u32 = env.mem.read(descriptor + 1);
    assert!(size >= 20, "Invalid block descriptor size");
    let bytes = env.mem.bytes_at(block.cast(), size).to_vec();
    let copy = env.mem.alloc(size);
    env.mem
        .bytes_at_mut(copy.cast(), size)
        .copy_from_slice(&bytes);
    env.mem.write(
        copy.cast::<u32>() + 1,
        (flags & !REFCOUNT_MASK) | BLOCK_NEEDS_FREE | 1,
    );
    if flags & BLOCK_HAS_COPY_DISPOSE != 0 {
        let helper: u32 = env.mem.read(descriptor + 2);
        let helper = GuestFunction::from_addr_with_thumb_bit(helper);
        let (): () = helper.call_from_host(env, (copy, block));
    }
    copy.cast_const()
}

pub fn _Block_release(env: &mut Environment, block: ConstVoidPtr) {
    if block.is_null() {
        return;
    }
    let words = block.cast::<u32>();
    let flags: u32 = env.mem.read(words + 1);
    if flags & BLOCK_IS_GLOBAL != 0 || flags & BLOCK_NEEDS_FREE == 0 {
        return;
    }
    if !remove_reference(env, (words + 1).cast_mut()) {
        return;
    }
    if flags & BLOCK_HAS_COPY_DISPOSE != 0 {
        let descriptor: crate::mem::ConstPtr<u32> = env.mem.read((words + 4).cast());
        let helper: u32 = env.mem.read(descriptor + 3);
        let helper = GuestFunction::from_addr_with_thumb_bit(helper);
        let (): () = helper.call_from_host(env, (block,));
    }
    env.mem.free(block.cast_mut());
}

fn copy_byref(env: &mut Environment, object: ConstVoidPtr) -> ConstVoidPtr {
    let original = object.cast::<u32>();
    let forwarded: crate::mem::MutPtr<u32> = env.mem.read((original + 1).cast());
    let flags: u32 = env.mem.read(forwarded + 2);
    if flags & BLOCK_NEEDS_FREE != 0 {
        add_reference(env, forwarded + 2);
        return forwarded.cast().cast_const();
    }
    let size: u32 = env.mem.read(forwarded + 3);
    assert!(size >= 16, "Invalid byref size");
    let bytes = env
        .mem
        .bytes_at(forwarded.cast().cast_const(), size)
        .to_vec();
    let copy = env.mem.alloc(size).cast::<u32>();
    env.mem
        .bytes_at_mut(copy.cast(), size)
        .copy_from_slice(&bytes);
    // One reference belongs to the stack scope, the other to the copied block.
    env.mem
        .write(copy + 2, (flags & !REFCOUNT_MASK) | BLOCK_NEEDS_FREE | 2);
    env.mem.write((copy + 1).cast(), copy);
    env.mem.write((forwarded + 1).cast(), copy);
    if flags & BLOCK_HAS_COPY_DISPOSE != 0 {
        let helper: u32 = env.mem.read(forwarded + 4);
        let helper = GuestFunction::from_addr_with_thumb_bit(helper);
        let (): () = helper.call_from_host(env, (copy, forwarded));
    }
    copy.cast().cast_const()
}

fn release_byref(env: &mut Environment, object: ConstVoidPtr) {
    let forwarded: crate::mem::MutPtr<u32> = env.mem.read((object.cast::<u32>() + 1).cast());
    let flags: u32 = env.mem.read(forwarded + 2);
    if flags & BLOCK_NEEDS_FREE == 0 || !remove_reference(env, forwarded + 2) {
        return;
    }
    if flags & BLOCK_HAS_COPY_DISPOSE != 0 {
        let helper: u32 = env.mem.read(forwarded + 5);
        let helper = GuestFunction::from_addr_with_thumb_bit(helper);
        let (): () = helper.call_from_host(env, (forwarded,));
    }
    env.mem.free(forwarded.cast());
}

/// `_Block_object_assign(destAddr, object, flags)`. Called by the
/// compiler-generated copy helper to retain `object` and store it at
/// `destAddr`. We perform the retain side-effect via `objc::retain`.
///
/// `flags` is a bitwise OR of `BLOCK_FIELD_IS_*` constants telling us what
/// the captured value is — an ObjC object, another block, or a `__block`
/// storage location. For ObjC objects and blocks we retain; for `__weak`
/// captures we do nothing (per the Blocks ABI).
fn _Block_object_assign(
    env: &mut Environment,
    dest_addr: MutVoidPtr,
    object: ConstVoidPtr,
    flags: i32,
) {
    // Byref copy helpers use BYREF_CALLER for their payload. They must not
    // recursively retain/copy that payload (nor retain weak captures).
    let value = if flags & (BLOCK_FIELD_IS_WEAK | BLOCK_BYREF_CALLER) != 0 {
        if flags & BLOCK_FIELD_IS_BYREF != 0 && !object.is_null() {
            copy_byref(env, object)
        } else {
            object
        }
    } else {
        match flags & 0xf {
            BLOCK_FIELD_IS_OBJECT => {
                retain(env, Ptr::from_bits(object.to_bits()));
                object
            }
            BLOCK_FIELD_IS_BLOCK => _Block_copy(env, object),
            BLOCK_FIELD_IS_BYREF if !object.is_null() => copy_byref(env, object),
            _ => object,
        }
    };
    env.mem.write(dest_addr.cast(), value);
}

fn _Block_object_dispose(env: &mut Environment, object: ConstVoidPtr, flags: i32) {
    if flags & BLOCK_BYREF_CALLER != 0 {
        return;
    }
    if flags & BLOCK_FIELD_IS_BYREF != 0 && !object.is_null() {
        release_byref(env, object);
    } else if flags & BLOCK_FIELD_IS_WEAK == 0 {
        match flags & 0xf {
            BLOCK_FIELD_IS_OBJECT => release(env, Ptr::from_bits(object.to_bits())),
            BLOCK_FIELD_IS_BLOCK => _Block_release(env, object),
            _ => (),
        }
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(_Block_copy(_)),
    export_c_func!(_Block_release(_)),
    export_c_func!(_Block_object_assign(_, _, _)),
    export_c_func!(_Block_object_dispose(_, _)),
];
#[cfg(test)]
mod tests {
    use super::{
        isa_is_block_class_descriptor, register_block_class_descriptor, valid_block_layout,
        BLOCK_HAS_COPY_DISPOSE, BLOCK_HAS_SIGNATURE, BLOCK_IS_GLOBAL,
    };

    #[test]
    fn validates_block_abi_layouts() {
        assert!(valid_block_layout(1, BLOCK_HAS_COPY_DISPOSE, 0x1001, 0x2000, 24));
        assert!(valid_block_layout(
            1,
            BLOCK_IS_GLOBAL | BLOCK_HAS_SIGNATURE,
            0x1000,
            0x2000,
            20,
        ));
        assert!(!valid_block_layout(1, 0, 0x1001, 0x2000, 24));
        assert!(!valid_block_layout(1, BLOCK_HAS_COPY_DISPOSE, 0, 0x2000, 24));
        assert!(!valid_block_layout(1, BLOCK_HAS_COPY_DISPOSE, 0x1001, 0x2000, 16));
    }

    #[test]
    fn block_class_descriptors_are_remembered_once_registered() {
        // An address no other test or dyld run can have registered.
        const DESCRIPTOR: u32 = 0x0bad_f00d;
        assert!(!isa_is_block_class_descriptor(DESCRIPTOR));
        register_block_class_descriptor(DESCRIPTOR);
        assert!(isa_is_block_class_descriptor(DESCRIPTOR));
        // Registering again must not duplicate the slot.
        register_block_class_descriptor(DESCRIPTOR);
        assert!(isa_is_block_class_descriptor(DESCRIPTOR));
        // Address 0 is never a class descriptor.
        register_block_class_descriptor(0);
        assert!(!isa_is_block_class_descriptor(0));
    }

    #[test]
    fn addresses_that_are_not_block_descriptors_are_rejected() {
        // A registered class address (or any other ordinary guest address) is
        // never a Blocks runtime class descriptor.
        assert!(!isa_is_block_class_descriptor(0x0030_1000));
        assert!(!isa_is_block_class_descriptor(0x0abc_def0));
        assert!(!isa_is_block_class_descriptor(0xffff_ffff));
    }
}
