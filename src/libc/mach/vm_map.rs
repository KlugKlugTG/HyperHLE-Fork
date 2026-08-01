/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Mach VM functions

use crate::dyld::{export_c_func, FunctionExports};
use crate::libc::mach::init::MACH_TASK_SELF;
use crate::libc::mach::port::mach_port_t;
use crate::libc::mach::thread_info::{kern_return_t, KERN_INVALID_ADDRESS, KERN_SUCCESS};
use crate::mem::{ConstPtr, MutPtr, Ptr, PAGE_SIZE, PAGE_SIZE_ALIGN_MASK};
use crate::Environment;
use std::collections::HashMap;

type vm_map_t = mach_port_t;
type vm_purgable_t = i32;
type mach_vm_address_t = u32;
type mach_vm_size_t = u32;
type vm_prot_t = i32;
type vm_inherit_t = u32;

const VM_PROT_READ: vm_prot_t = 1;
const VM_PROT_WRITE: vm_prot_t = 2;
const VM_PROT_EXECUTE: vm_prot_t = 4;

#[derive(Default)]
pub struct State {
    /// Keeping track of `vm_allocate` allocations
    allocations: HashMap<mach_vm_address_t, mach_vm_size_t>,
}

pub fn vm_allocate(
    env: &mut Environment,
    target_task: vm_map_t,
    address_ptr: MutPtr<mach_vm_address_t>,
    size: mach_vm_size_t,
    flags: i32, // in other docs it is defined as `anywhere: boolean_t`
) -> kern_return_t {
    assert_eq!(target_task, MACH_TASK_SELF);
    assert_eq!(flags, 1); // TRUE

    // `size is always rounded up to an integral number of pages`
    let new_size = if !size.is_multiple_of(PAGE_SIZE) {
        size + PAGE_SIZE - (size % PAGE_SIZE)
    } else {
        size
    };
    // touchHLE delegates page-granularity Mach VM allocations to the
    // standard guest heap allocator. This is fine in practice — apps
    // call vm_allocate to obtain page-aligned scratch buffers, which
    // env.mem.alloc does honour — but it does mean we can't enforce
    // protection bits or sparse mappings. Demoted to debug because Mono
    // / Boehm GC use vm_allocate as their primary heap source and the
    // log_once line was the very first noisy entry in long sessions.
    log_dbg!("vm_allocate() implemented atop standard allocator");
    let allocated = env.mem.alloc(new_size);
    let address = allocated.to_bits();
    assert!(address & PAGE_SIZE_ALIGN_MASK == 0);
    env.mem.write(address_ptr, address);

    assert!(!env.libc_state.mach_vm.allocations.contains_key(&address));
    // Note: we keep track of the original size,
    // not the one what was actually allocated!
    env.libc_state.mach_vm.allocations.insert(address, size);

    KERN_SUCCESS
}

fn vm_deallocate(
    env: &mut Environment,
    target_task: vm_map_t,
    address: mach_vm_address_t,
    size: mach_vm_size_t,
) -> kern_return_t {
    assert_eq!(target_task, MACH_TASK_SELF);
    log_dbg!("vm_deallocate() implemented atop standard allocator");

    // The guest may ask us to free a region we never handed out via
    // vm_allocate (a double free, a region obtained by other means, or a
    // bogus pointer). A real Mach kernel returns KERN_INVALID_ADDRESS in
    // that case rather than aborting the task, so mirror that instead of
    // panicking on the missing map entry.
    let Some(&tracked_size) = env.libc_state.mach_vm.allocations.get(&address) else {
        log!(
            "Warning: vm_deallocate({:#x}, {:#x}) for an address that was not allocated via vm_allocate; returning KERN_INVALID_ADDRESS.",
            address,
            size
        );
        return KERN_INVALID_ADDRESS;
    };

    // We record the original requested size in `vm_allocate`, but the guest
    // is free to pass either the original size or the page-rounded size it
    // was effectively given. Accept both.
    let rounded_tracked_size = if !tracked_size.is_multiple_of(PAGE_SIZE) {
        tracked_size + PAGE_SIZE - (tracked_size % PAGE_SIZE)
    } else {
        tracked_size
    };
    if size != tracked_size && size != rounded_tracked_size {
        log!(
            "Warning: vm_deallocate({:#x}, {:#x}) size mismatch (region was allocated with size {:#x}); freeing the whole region anyway.",
            address,
            size,
            tracked_size
        );
    }

    env.mem.free(Ptr::from_bits(address));
    env.libc_state.mach_vm.allocations.remove(&address);

    KERN_SUCCESS
}

/// `kern_return_t vm_remap(vm_map_t target_task, vm_address_t *target_address,
/// vm_size_t size, vm_address_t mask, int flags, vm_map_t src_task,
/// vm_address_t src_address, boolean_t copy, vm_prot_t *cur_protection,
/// vm_prot_t *max_protection, vm_inherit_t inheritance)`
///
/// Real Mach `vm_remap` maps the physical pages backing a region of one task's
/// address space into another (or the same) task, optionally sharing them.
/// touchHLE has a single flat guest address space and no notion of shared
/// physical pages, so we approximate it: allocate a fresh page-aligned region
/// and copy the source bytes into it. This is what callers like the Mono/Boehm
/// GC actually need to keep running — a valid, readable mapping of the same
/// contents — even though writes won't be reflected back into the source.
#[allow(clippy::too_many_arguments)]
fn vm_remap(
    env: &mut Environment,
    target_task: vm_map_t,
    target_address: MutPtr<mach_vm_address_t>,
    size: mach_vm_size_t,
    _mask: mach_vm_address_t,
    _flags: i32,
    _src_task: vm_map_t,
    src_address: mach_vm_address_t,
    copy: i32,
    cur_protection: MutPtr<vm_prot_t>,
    max_protection: MutPtr<vm_prot_t>,
    _inheritance: vm_inherit_t,
) -> kern_return_t {
    assert_eq!(target_task, MACH_TASK_SELF);

    if size == 0 || src_address == 0 {
        log!(
            "Warning: vm_remap(src={:#x}, size={:#x}): invalid argument; returning KERN_INVALID_ADDRESS.",
            src_address,
            size
        );
        return KERN_INVALID_ADDRESS;
    }

    // `size` is always rounded up to an integral number of pages.
    let new_size = if !size.is_multiple_of(PAGE_SIZE) {
        size + PAGE_SIZE - (size % PAGE_SIZE)
    } else {
        size
    };

    log_dbg!(
        "vm_remap(src={:#x}, size={:#x}, copy={}) approximated by allocate + copy",
        src_address,
        size,
        copy
    );

    let allocated = env.mem.alloc(new_size);
    let address = allocated.to_bits();
    assert!(address & PAGE_SIZE_ALIGN_MASK == 0);

    // Copy the source region's bytes so the new mapping reads back the same
    // data. We copy the page-rounded length so the trailing partial page is
    // also valid.
    let copy_len = new_size.min(u32::MAX);
    let src: ConstPtr<u8> = Ptr::from_bits(src_address);
    let dst: MutPtr<u8> = Ptr::from_bits(address);
    let bytes: Vec<u8> = env.mem.bytes_at(src, copy_len).to_vec();
    env.mem
        .bytes_at_mut(dst, copy_len)
        .copy_from_slice(&bytes);

    env.mem.write(target_address, address);
    if !cur_protection.is_null() {
        env.mem
            .write(cur_protection, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE);
    }
    if !max_protection.is_null() {
        env.mem
            .write(max_protection, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE);
    }

    assert!(!env.libc_state.mach_vm.allocations.contains_key(&address));
    env.libc_state.mach_vm.allocations.insert(address, size);

    KERN_SUCCESS
}

fn vm_purgable_control(
    _env: &mut Environment,
    target_task: vm_map_t,
    address: mach_vm_address_t,
    control: vm_purgable_t,
    state: MutPtr<vm_purgable_t>,
) -> kern_return_t {
    assert_eq!(target_task, MACH_TASK_SELF);
    log!("TODO: vm_purgable_control({target_task:#x}, {address:#x}, {control:#x}, {state:?})");
    KERN_SUCCESS
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(vm_allocate(_, _, _, _)),
    export_c_func!(vm_deallocate(_, _, _)),
    export_c_func!(vm_remap(_, _, _, _, _, _, _, _, _, _, _)),
    export_c_func!(vm_purgable_control(_, _, _, _)),
];
