/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! This is separated out into its own package so that we can avoid rebuilding
//! dynarmic more often than necessary, and to improve build-time parallelism.

// Allow the crate to have a non-snake-case name (HyperHLE).
// This also allows items in the crate to have non-snake-case names.
#![allow(non_snake_case)]

/// Opaque type from C
#[allow(non_camel_case_types)]
pub type HyperHLE_DynarmicWrapper = std::ffi::c_void;
/// Opaque type from Rust (this is the `Mem` type from the main crate, but
/// `c_void` is used here to avoid depending on it directly)
#[allow(non_camel_case_types)]
pub type HyperHLE_Mem = std::ffi::c_void;

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Debug)]
pub struct HyperHLE_DynarmicContext {
    pub regs: [u32; 16],
    pub extregs: [u32; 64],
    pub cpsr: u32,
    pub fpscr: u32,
}

impl Default for HyperHLE_DynarmicContext {
    fn default() -> Self {
        Self {
            regs: [0; 16],
            extregs: [0; 64],
            cpsr: 0,
            fpscr: 0,
        }
    }
}

impl HyperHLE_DynarmicContext {
    pub fn new() -> Self {
        Self::default()
    }
}
type VAddr = u32;

// Import functions from lib.cpp, see build.rs. Note that lib.cpp depends on
// some functions being exported from Rust, but those are in the main crate.
extern "C" {
    pub fn HyperHLE_DynarmicWrapper_new(
        dynamic_memory_access_ptr: *mut std::ffi::c_void,
        null_page_count: usize,
    ) -> *mut HyperHLE_DynarmicWrapper;
    pub fn HyperHLE_DynarmicWrapper_delete(cpu: *mut HyperHLE_DynarmicWrapper);
    pub fn HyperHLE_DynarmicWrapper_regs_const(cpu: *const HyperHLE_DynarmicWrapper) -> *const u32;
    pub fn HyperHLE_DynarmicWrapper_regs_mut(cpu: *mut HyperHLE_DynarmicWrapper) -> *mut u32;
    pub fn HyperHLE_DynarmicWrapper_cpsr(cpu: *const HyperHLE_DynarmicWrapper) -> u32;
    pub fn HyperHLE_DynarmicWrapper_set_cpsr(cpu: *mut HyperHLE_DynarmicWrapper, cpsr: u32);
    pub fn HyperHLE_DynarmicWrapper_swap_context(
        cpu: *mut HyperHLE_DynarmicWrapper,
        context: *mut HyperHLE_DynarmicContext,
    );
    pub fn HyperHLE_DynarmicWrapper_invalidate_cache_range(
        cpu: *mut HyperHLE_DynarmicWrapper,
        start: VAddr,
        size: u32,
    );
    pub fn HyperHLE_DynarmicWrapper_run_or_step(
        cpu: *mut HyperHLE_DynarmicWrapper,
        mem: *mut HyperHLE_Mem,
        ticks: Option<&mut u64>,
    ) -> i32;

}
