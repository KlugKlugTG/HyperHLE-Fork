/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Host CPU tuning so touchHLE makes full use of the host device's power.
//!
//! touchHLE's guest CPU emulation (dynarmic) runs on a single host thread, and
//! almost everything else (the framework implementations, the OpenGL ES
//! command translation, the main run loop) runs cooperatively on that very
//! same thread. So the throughput of the whole emulator is gated by how fast
//! that one thread runs.
//!
//! On a big.LITTLE host — which is every Android phone touchHLE targets,
//! e.g. a MediaTek Helio G99 with 2× Cortex-A76 "big" cores and 6×
//! Cortex-A55 "little" cores — the OS scheduler is free to, and frequently
//! does, migrate this hot thread onto a slow efficiency core. That caps the
//! frame rate well below what the device is capable of. To get full
//! performance we ask the OS to keep the emulation thread on the fastest
//! available cores and to schedule it at an elevated priority.
//!
//! This is entirely best-effort: every operation here is allowed to fail
//! silently (logging at debug level only), because none of it is required for
//! correctness and the relevant syscalls are not available on every platform
//! or may be denied by the sandbox.

/// Pin the calling thread (the emulation thread) to the host's performance
/// ("big") cores and raise its scheduling priority.
///
/// Safe to call exactly once at startup from the thread that runs the guest
/// CPU. A no-op on platforms where we don't have an implementation.
pub fn pin_emulation_thread_to_performance_cores() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    linux::tune_current_thread();

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    log_dbg!(
        "Host CPU tuning: no performance-core pinning implemented for this \
         platform; relying on the OS scheduler."
    );
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux {
    /// Raise the niceness/priority of the current thread so the scheduler
    /// favours it. Negative nice values mean higher priority; -10 is a strong
    /// but not reckless boost that doesn't require real-time scheduling (which
    /// is usually denied to apps anyway). Best-effort.
    fn raise_priority() {
        // PRIO_PROCESS with who == 0 means "the calling thread" on Linux
        // (per-thread niceness), not the whole process.
        let res = unsafe { ::libc::setpriority(::libc::PRIO_PROCESS, 0, -10) };
        if res == 0 {
            log_dbg!("Host CPU tuning: raised emulation thread priority (nice -10).");
        } else {
            log_dbg!(
                "Host CPU tuning: could not raise emulation thread priority \
                 (this is normal if the OS denies it)."
            );
        }
    }

    /// Figure out which logical CPUs are the host's fastest cores.
    ///
    /// We read each CPU's maximum frequency from sysfs
    /// (`/sys/devices/system/cpu/cpuN/cpufreq/cpuinfo_max_freq`) and select all
    /// CPUs whose max frequency equals the highest observed value. On a
    /// big.LITTLE SoC this picks exactly the "big" cluster (e.g. the two
    /// Cortex-A76 cores on a Helio G99). If the information isn't available we
    /// return an empty list and the caller leaves affinity untouched.
    fn detect_performance_cpus() -> Vec<usize> {
        let online = num_configured_cpus();
        if online == 0 {
            return Vec::new();
        }

        let mut max_freqs: Vec<(usize, u64)> = Vec::with_capacity(online);
        for cpu in 0..online {
            let path = format!(
                "/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq"
            );
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(freq) = contents.trim().parse::<u64>() {
                    max_freqs.push((cpu, freq));
                }
            }
        }

        // If we couldn't read frequencies for (almost) all CPUs, the heuristic
        // isn't trustworthy, so bail out and leave scheduling to the OS.
        if max_freqs.len() < online {
            return Vec::new();
        }

        let highest = max_freqs.iter().map(|&(_, f)| f).max().unwrap_or(0);
        if highest == 0 {
            return Vec::new();
        }

        let fast: Vec<usize> = max_freqs
            .iter()
            .filter(|&&(_, f)| f == highest)
            .map(|&(cpu, _)| cpu)
            .collect();

        // If every CPU has the same max frequency the host isn't big.LITTLE
        // (or all cores are equally fast); there's nothing to gain by
        // restricting affinity, and doing so would only reduce the cores the
        // scheduler can use, so treat it as "no preference".
        if fast.len() == online {
            return Vec::new();
        }

        fast
    }

    /// Number of logical CPUs configured on the host, via sysconf. Returns 0 if
    /// it can't be determined.
    fn num_configured_cpus() -> usize {
        let n = unsafe { ::libc::sysconf(::libc::_SC_NPROCESSORS_CONF) };
        if n < 1 {
            0
        } else {
            n as usize
        }
    }

    /// Restrict the current thread to run only on the given logical CPUs.
    /// Best-effort.
    fn set_affinity(cpus: &[usize]) {
        // SAFETY: cpu_set_t is a plain bitset; we zero it, set the bits we want
        // and hand a pointer to sched_setaffinity. who == 0 targets the
        // calling thread.
        unsafe {
            let mut set: ::libc::cpu_set_t = std::mem::zeroed();
            ::libc::CPU_ZERO(&mut set);
            for &cpu in cpus {
                // CPU_SETSIZE caps how many CPUs fit in the set; ignore any
                // (implausibly high) index beyond it.
                if cpu < ::libc::CPU_SETSIZE as usize {
                    ::libc::CPU_SET(cpu, &mut set);
                }
            }
            let res = ::libc::sched_setaffinity(
                0,
                std::mem::size_of::<::libc::cpu_set_t>(),
                &set,
            );
            if res == 0 {
                log!(
                    "Host CPU tuning: pinned emulation thread to performance \
                     core(s) {:?}.",
                    cpus
                );
            } else {
                log_dbg!(
                    "Host CPU tuning: sched_setaffinity failed; leaving \
                     emulation thread affinity to the OS scheduler."
                );
            }
        }
    }

    pub fn tune_current_thread() {
        let fast = detect_performance_cpus();
        if fast.is_empty() {
            log_dbg!(
                "Host CPU tuning: no distinct performance-core cluster \
                 detected; leaving affinity to the OS scheduler."
            );
        } else {
            set_affinity(&fast);
        }
        raise_priority();
    }
}
