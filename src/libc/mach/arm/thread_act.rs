/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Mach thread actions for ARM arch.
//!
//! Apple reference (Mach kernel):
//! - thread_get_state:
//!   <https://developer.apple.com/documentation/kernel/1538090-thread_get_state>
//! - ARM thread state layout (`<mach/arm/thread_status.h>`):
//!   <https://opensource.apple.com/source/xnu/xnu-7195.81.3/osfmk/mach/arm/thread_status.h.auto.html>
//!
//! Mono's Boehm GC suspends every thread and uses `thread_get_state()` to read
//! their general-purpose registers so it can scan them as conservative GC
//! roots. If `thread_get_state()` claims success without actually filling the
//! state buffer, the GC reads zero/garbage register values and tries to scan
//! memory starting at address 0, which trips touchHLE's null-page check.

use crate::cpu::Cpu;
use crate::dyld::{export_c_func, FunctionExports};
use crate::environment::ThreadBlock;
use crate::libc::mach::core_types::integer_t;
use crate::libc::mach::port::{mach_port_t, MACH_PORT_DEAD, MACH_PORT_NULL};
use crate::libc::mach::thread_info::{
    kern_return_t, mach_msg_type_number_t, thread_state_flavor_t, thread_state_t, KERN_SUCCESS,
};
use crate::mem::{guest_size_of, MutPtr, SafeRead};
use crate::{Environment, ThreadId};

type thread_act_t = mach_port_t;

const ARM_THREAD_STATE: thread_state_flavor_t = 1;
const MACHINE_THREAD_STATE: thread_state_flavor_t = ARM_THREAD_STATE;

/// Layout of `_STRUCT_ARM_THREAD_STATE` from `<mach/arm/thread_status.h>`:
/// 13 general-purpose registers + sp + lr + pc + cpsr.
#[repr(C, packed)]
struct arm_thread_state {
    r: [u32; 13],
    sp: u32,
    lr: u32,
    pc: u32,
    cpsr: u32,
}
unsafe impl SafeRead for arm_thread_state {}

fn thread_suspend(env: &mut Environment, target_thread: thread_act_t) -> kern_return_t {
    assert!(target_thread != MACH_PORT_NULL && target_thread != MACH_PORT_DEAD);
    // Expected `thread send right` is thread_id + 1. See `mach_thread_self()`.
    env.suspend_thread((target_thread - 1) as ThreadId);
    KERN_SUCCESS
}

fn thread_resume(env: &mut Environment, target_thread: thread_act_t) -> kern_return_t {
    assert!(target_thread != MACH_PORT_NULL && target_thread != MACH_PORT_DEAD);
    // Expected `thread send right` is thread_id + 1. See `mach_thread_self()`.
    env.resume_thread((target_thread - 1) as ThreadId);
    KERN_SUCCESS
}

fn thread_get_state(
    env: &mut Environment,
    target_thread: thread_act_t,
    flavor: thread_state_flavor_t,
    old_state: thread_state_t,
    old_state_count: MutPtr<mach_msg_type_number_t>,
) -> kern_return_t {
    assert!(target_thread != MACH_PORT_NULL && target_thread != MACH_PORT_DEAD);
    assert_eq!(flavor, MACHINE_THREAD_STATE);

    let out_size_available = env.mem.read(old_state_count);
    let out_size_expected = guest_size_of::<arm_thread_state>() / guest_size_of::<integer_t>();
    assert!(out_size_expected <= out_size_available);

    // Expected `thread send right` is thread_id + 1. See `mach_thread_self()`.
    let thread_id = (target_thread - 1) as ThreadId;
    // Mono's GC always suspends a thread before reading its state, so this
    // assertion catches incorrect usage rather than restricting valid callers.
    assert!(matches!(
        env.threads[thread_id].blocked_by,
        ThreadBlock::Suspended(_, _)
    ));
    let ctx = env.threads[thread_id].guest_context.as_ref().unwrap();
    let state = arm_thread_state {
        r: ctx.regs[..13].try_into().unwrap(),
        sp: ctx.regs[Cpu::SP],
        lr: ctx.regs[Cpu::LR],
        pc: ctx.regs[Cpu::PC],
        cpsr: ctx.cpsr,
    };
    env.mem.write(old_state.cast(), state);
    env.mem.write(old_state_count, out_size_expected);

    KERN_SUCCESS
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(thread_suspend(_)),
    export_c_func!(thread_resume(_)),
    export_c_func!(thread_get_state(_, _, _, _)),
];
