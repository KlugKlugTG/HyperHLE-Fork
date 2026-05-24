/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `task`

use crate::dyld::{export_c_func, FunctionExports};
use crate::libc::mach::arm::vm_types::vm_size_t;
use crate::libc::mach::core_types::{integer_t, natural_t};
use crate::libc::mach::init::MACH_TASK_SELF;
use crate::libc::mach::policy::policy_t;
use crate::libc::mach::port::mach_port_t;
use crate::libc::mach::thread_info::{kern_return_t, mach_msg_type_number_t, KERN_SUCCESS};
use crate::libc::mach::time_value::time_value_t;
use crate::mem::{guest_size_of, MutPtr, SafeRead};
use crate::Environment;

#[repr(C, packed)]
struct task_basic_info {
    suspend_count: integer_t,
    virtual_size: vm_size_t,
    resident_size: vm_size_t,
    user_time: time_value_t,
    system_time: time_value_t,
    policy: policy_t,
}
unsafe impl SafeRead for task_basic_info {}

#[allow(non_camel_case_types)]
type task_name_t = mach_port_t;
#[allow(non_camel_case_types)]
type task_flavor_t = natural_t;
#[allow(non_camel_case_types)]
type task_info_t = MutPtr<integer_t>;

const TASK_BASIC_INFO: task_flavor_t = 4;

#[allow(dead_code)]
const POLICY_NULL: policy_t = 0;
const POLICY_TIMESHARE: policy_t = 1;
#[allow(dead_code)]
const POLICY_RR: policy_t = 2;
#[allow(dead_code)]
const POLICY_FIFO: policy_t = 4;

/// Query the host process for actual CPU-time and memory statistics so that
/// guest apps (e.g. Terraria’s in-game memory counter) show live data.
fn get_task_basic_info() -> task_basic_info {
    let mut user_seconds: integer_t = 0;
    let mut user_microseconds: integer_t = 0;
    let mut system_seconds: integer_t = 0;
    let mut system_microseconds: integer_t = 0;
    let mut virtual_size: vm_size_t = 0;
    let mut resident_size: vm_size_t = 0;

    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
            let usage = unsafe { usage.assume_init() };
            user_seconds = usage.ru_utime.tv_sec as integer_t;
            user_microseconds = usage.ru_utime.tv_usec as integer_t;
            system_seconds = usage.ru_stime.tv_sec as integer_t;
            system_microseconds = usage.ru_stime.tv_usec as integer_t;

            #[cfg(target_os = "linux")]
            {
                resident_size = (usage.ru_maxrss as u64)
                    .saturating_mul(1024)
                    .min(u32::MAX as u64) as vm_size_t;
            }
            #[cfg(not(target_os = "linux"))]
            {
                // macOS / BSD report ru_maxrss in bytes already.
                resident_size = (usage.ru_maxrss as u64).min(u32::MAX as u64) as vm_size_t;
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let mut parts = statm.split_whitespace();
            // Field 1: total program size (pages)
            if let Some(size_pages) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
                virtual_size = size_pages
                    .saturating_mul(page_size)
                    .min(u32::MAX as u64) as vm_size_t;
            }
            // Field 2: resident set size (pages)
            if let Some(res_pages) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
                resident_size = res_pages
                    .saturating_mul(page_size)
                    .min(u32::MAX as u64) as vm_size_t;
            }
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Memory::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        let mut pmc = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
        let handle = windows_sys::Win32::System::Threading::GetCurrentProcess();
        if unsafe {
            GetProcessMemoryInfo(
                handle,
                pmc.as_mut_ptr(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        } != 0
        {
            let pmc = unsafe { pmc.assume_init() };
            resident_size = (pmc.WorkingSetSize as u64).min(u32::MAX as u64) as vm_size_t;
            virtual_size = (pmc.PagefileUsage as u64).min(u32::MAX as u64) as vm_size_t;
        }

        // User / system time via GetProcessTimes
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        let mut creation = std::mem::MaybeUninit::<windows_sys::Win32::Foundation::FILETIME>::zeroed();
        let mut exit = std::mem::MaybeUninit::<windows_sys::Win32::Foundation::FILETIME>::zeroed();
        let mut kernel = std::mem::MaybeUninit::<windows_sys::Win32::Foundation::FILETIME>::zeroed();
        let mut user = std::mem::MaybeUninit::<windows_sys::Win32::Foundation::FILETIME>::zeroed();
        if unsafe { GetProcessTimes(handle, creation.as_mut_ptr(), exit.as_mut_ptr(), kernel.as_mut_ptr(), user.as_mut_ptr()) } != 0 {
            let user_ft = unsafe { user.assume_init() };
            let kernel_ft = unsafe { kernel.assume_init() };
            // FILETIME is 100-nanosecond intervals since 1601.
            let user_100ns = ((user_ft.dwHighDateTime as u64) << 32) | (user_ft.dwLowDateTime as u64);
            let kernel_100ns = ((kernel_ft.dwHighDateTime as u64) << 32) | (kernel_ft.dwLowDateTime as u64);
            let user_us = user_100ns / 10;
            let kernel_us = kernel_100ns / 10;
            user_seconds = (user_us / 1_000_000) as integer_t;
            user_microseconds = (user_us % 1_000_000) as integer_t;
            system_seconds = (kernel_us / 1_000_000) as integer_t;
            system_microseconds = (kernel_us % 1_000_000) as integer_t;
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Fallback for unknown platforms — not a stub, just zeroes until a
        // platform-specific path is added.
    }

    task_basic_info {
        suspend_count: 0,
        virtual_size,
        resident_size,
        user_time: time_value_t {
            seconds: user_seconds,
            microseconds: user_microseconds,
        },
        system_time: time_value_t {
            seconds: system_seconds,
            microseconds: system_microseconds,
        },
        policy: POLICY_TIMESHARE,
    }
}

fn task_info(
    env: &mut Environment,
    target_task: task_name_t,
    flavor: task_flavor_t,
    task_info_out: task_info_t,
    task_info_out_cnt: MutPtr<mach_msg_type_number_t>,
) -> kern_return_t {
    assert_eq!(target_task, MACH_TASK_SELF);
    assert_eq!(flavor, TASK_BASIC_INFO);
    let out_size_available = env.mem.read(task_info_out_cnt);
    let out_size_expected = guest_size_of::<task_basic_info>() / guest_size_of::<integer_t>();
    assert!(out_size_expected <= out_size_available);

    let info = get_task_basic_info();
    env.mem.write(task_info_out.cast(), info);
    KERN_SUCCESS
}

pub const FUNCTIONS: FunctionExports = &[export_c_func!(task_info(_, _, _, _))];
