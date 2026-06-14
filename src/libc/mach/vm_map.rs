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
use crate::mem::{MutPtr, Ptr, PAGE_SIZE, PAGE_SIZE_ALIGN_MASK};
use crate::Environment;
use std::collections::HashMap;

type vm_map_t = mach_port_t;
type vm_purgable_t = i32;
type mach_vm_address_t = u32;
type mach_vm_size_t = u32;

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
    export_c_func!(vm_purgable_control(_, _, _, _)),
];
