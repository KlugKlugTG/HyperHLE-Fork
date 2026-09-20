/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Native crash diagnostics.
//!
//! touchHLE's own errors and guest errors are always logged, but a *host*
//! crash (SIGSEGV/SIGBUS/SIGILL from e.g. a JIT bug or a graphics driver)
//! kills the process silently, which makes bug reports undiagnosable ("it
//! just closes with no error"). These handlers write a one-line marker with
//! the fatal signal and faulting address to the same log file the user can
//! share, then restore the default disposition and re-raise so the platform's
//! own crash reporting (Android tombstones etc.) still works.

/// Append a message to the log file (and stderr). Safe to call from a panic
/// hook; uses file-level locking via try_lock so re-entrant panics don't
/// deadlock — on contention the message is dropped rather than deadlocked.
#[cfg(unix)]
fn raw_fd() -> i32 {
    imp::log_fd()
}

#[cfg(not(unix))]
fn raw_fd() -> i32 {
    -1
}

#[cfg(unix)]
fn write_size_t(n: usize) -> libc::size_t {
    n as libc::size_t
}
#[cfg(windows)]
fn write_size_t(n: usize) -> libc::c_uint {
    n as libc::c_uint
}

pub fn append_to_log(msg: &str) {
    use std::io::Write;
    // First try the normal locked path...
    if let Ok(mut log_file) = crate::log::get_log_file().try_lock() {
        let _ = log_file.write_all(msg.as_bytes());
        let _ = log_file.write_all(b"\n");
        let _ = log_file.flush();
    }
    // ...but if the mutex is held by some other thread (very likely right
    // before a crash, since logging is what the guest threads spend their
    // time on), fall back to a raw fd write. O_APPEND-style writes of whole
    // small lines are atomic enough for diagnostics.
    let fd = raw_fd();
    if fd >= 0 {
        let mut line = msg.as_bytes().to_vec();
        line.push(b'\n');
        let mut written = 0usize;
        while written < line.len() {
            let n = unsafe {
                libc::write(
                    fd,
                    line.as_ptr().add(written) as *const libc::c_void,
                    write_size_t(line.len() - written),
                )
            };
            if n <= 0 {
                break;
            }
            written += n as usize;
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = std::io::stderr().write_all(msg.as_bytes());
        let _ = std::io::stderr().write_all(b"\n");
    }
}

/// Install a Rust panic hook that mirrors panic messages into the touchHLE
/// log file. The default hook only writes to stderr/logcat, which users
/// rarely capture, so panics look like silent aborts (especially on Android,
/// where a panic unwinding out of a guest-thread coroutine ends in
/// SIGABRT — see the FATAL SIGNAL marker in the log).
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let msg = format!(
            "touchHLE: PANIC in thread \"{}\" at {}: {}\n(panic is followed by unwinding; if this appears right before a FATAL SIGNAL line, the panic crossed a coroutine boundary and aborted the process)",
            thread_name,
            info.location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "<unknown>".to_string()),
            info.payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string()),
        );
        append_to_log(&msg);
    }));
}

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    /// Raw fd of the touchHLE log file, so the signal handler (which cannot
    /// safely use the `Mutex<File>` in `log::get_log_file()`) can append to it.
    static LOG_FD: AtomicI32 = AtomicI32::new(-1);

    /// Runtime-resolved `backtrace()` (bionic exposes it on Android 12+,
    /// glibc on desktop; resolving via dlsym avoids build-target cfg).
    static BACKTRACE_SYM: AtomicUsize = AtomicUsize::new(0);

    pub fn resolve_backtrace() {
        unsafe {
            let sym = libc::dlsym(
                libc::RTLD_DEFAULT,
                b"backtrace\0".as_ptr() as *const libc::c_char,
            );
            BACKTRACE_SYM.store(sym as usize, Ordering::SeqCst);
        }
    }

    pub fn native_backtrace_lines() -> String {
        let fptr = BACKTRACE_SYM.load(Ordering::Relaxed);
        if fptr == 0 {
            return "(native backtrace() not available on this device)\n".to_string();
        }
        type BacktraceFn = unsafe extern "C" fn(*mut *mut libc::c_void, libc::c_int) -> libc::c_int;
        let bt: BacktraceFn = unsafe { std::mem::transmute(fptr) };
        let mut addrs = [std::ptr::null_mut::<libc::c_void>(); 64];
        let n = unsafe { bt(addrs.as_mut_ptr(), 64) };
        let mut lines = format!("native backtrace ({} frames):\n", n);
        for i in 0..n as usize {
            lines.push_str(&format!("  #{}: {:#x}\n", i, addrs[i] as usize));
        }
        lines.push_str(&maps_for(&addrs[..n as usize]));
        lines
    }

    /// Dump the /proc/self/maps lines whose ranges contain one of `addrs`,
    /// so each backtrace frame can be attributed to a loaded library.
    /// Only open/read/close are used — good enough in an abort handler.
    fn maps_for(addrs: &[*mut libc::c_void]) -> String {
        let path = b"/proc/self/maps\0";
        let fd = unsafe { libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDONLY) };
        if fd < 0 {
            return String::new();
        }
        // /proc/self/maps on Android with a loaded GPU driver stack is large;
        // grow until EOF so late-loaded libs (GLES driver, openal) are present.
        let mut buf: Vec<u8> = vec![0u8; 1 << 16];
        let mut off = 0usize;
        loop {
            if off >= buf.len() {
                buf.resize(buf.len() * 2, 0);
            }
            let n = unsafe {
                libc::read(
                    fd,
                    buf.as_mut_ptr().add(off) as *mut libc::c_void,
                    buf.len() - off,
                )
            };
            if n <= 0 {
                break;
            }
            off += n as usize;
        }
        unsafe { libc::close(fd) };
        let text = String::from_utf8_lossy(&buf[..off]).into_owned();
        let mut out = String::from("relevant /proc/self/maps entries:\n");
        for line in text.lines() {
            // "start-end perms offset dev inode path"
            let mut it = line.splitn(2, ' ');
            if let Some(range) = it.next() {
                let mut parts = range.split('-');
                let (Some(start), Some(end)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let (Ok(start), Ok(end)) = (
                    usize::from_str_radix(start, 16),
                    usize::from_str_radix(end, 16),
                ) else {
                    continue;
                };
                if addrs.iter().any(|a| {
                    let a = *a as usize;
                    a >= start && a < end
                }) {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn log_fd() -> i32 {
        LOG_FD.load(Ordering::SeqCst)
    }

    /// Register the raw fd of the log file. Called after the log file is
    /// created; async-signal-safe `write(2)` then targets it.
    pub fn set_log_fd(fd: i32) {
        LOG_FD.store(fd, Ordering::SeqCst);
    }

    const NAME_SEGV: &[u8] = b"SIGSEGV\0";
    const NAME_BUS: &[u8] = b"SIGBUS\0";
    const NAME_ILL: &[u8] = b"SIGILL\0";
    const NAME_ABORT: &[u8] = b"SIGABRT\0";

    extern "C" fn handler(sig: libc::c_int, info: *mut libc::siginfo_t, _uc: *mut libc::c_void) {
        let name: &[u8] = match sig {
            libc::SIGSEGV => NAME_SEGV,
            libc::SIGBUS => NAME_BUS,
            libc::SIGILL => NAME_ILL,
            libc::SIGABRT => NAME_ABORT,
            _ => b"SIGNAL\0",
        };
        // si_addr is the faulting memory address for SIGSEGV/SIGBUS/SIGILL.
        let addr = unsafe {
            match sig {
                libc::SIGABRT => 0,
                _ => (*(info as *const libc::siginfo_t)).si_addr() as usize,
            }
        };
        let location = if sig == libc::SIGABRT {
            " (abort; no fault address)".to_string()
        } else {
            format!(" at address {:#x}", addr)
        };
        let msg = format!(
            "touchHLE: FATAL: native host crash: {}{} — the signal alone does not identify the root cause. Check preceding panic, loader and guest-fault messages. The process will now terminate.\n",
            std::str::from_utf8(&name[..name.len() - 1]).unwrap_or("SIGNAL"),
            location
        );
        let msg = format!(
            "{}last guest PC: {:#x}, LR: {:#x}\n",
            msg,
            crate::environment::LAST_GUEST_PC.load(Ordering::Relaxed),
            crate::environment::LAST_GUEST_LR.load(Ordering::Relaxed)
        );
        // Identify the aborting OS thread and dump a NATIVE backtrace:
        // the guest PC ring only tracks the emulation thread, so if the
        // crash came from a worker (audio/net/JIT helper), this is the only
        // way to see where it actually died.
        let msg = format!(
            "{}aborting thread: name={:?} tid={}\n",
            msg,
            std::thread::current().name().unwrap_or("<unnamed>"),
            // libc::gettid is Linux-only; pthread_self works everywhere.
            unsafe { libc::pthread_self() as u64 }
        );
        let msg = format!("{}{}", msg, native_backtrace_lines());
        let msg = format!(
            "{}recent guest PCs:{}\n",
            msg,
            (0..32)
                .map(|i| {
                    let idx = (crate::environment::GUEST_PC_RING_IDX
                        .load(Ordering::Relaxed)
                        .wrapping_sub(1 - i as usize))
                        % 32;
                    format!(
                        " {:#x}",
                        crate::environment::GUEST_PC_RING[idx].load(Ordering::Relaxed)
                    )
                })
                .collect::<String>()
        );
        let bytes = msg.as_bytes();
        unsafe {
            // Best-effort write to both stderr and the log file. write(2) is
            // async-signal-safe.
            let _ = libc::write(2, bytes.as_ptr() as *const libc::c_void, bytes.len());
            let fd = LOG_FD.load(Ordering::SeqCst);
            if fd >= 0 {
                let _ = libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len());
            }
            // On Android, the most common cause of this abort is a JNI
            // fatal error raised by ART ("JNI DETECTED ERROR IN APPLICATION:
            // ..."), whose message only goes to logcat. Fork + exec
            // `logcat -d --pid=<ours>` to append our own log entries (the
            // JNI error text included) to the log file before dying.
            #[cfg(target_os = "android")]
            {
                let my_pid = libc::getpid();
                let child = libc::fork();
                if child == 0 {
                    // Child: redirect stdout/stderr into the log file and
                    // dump the logcat ring buffer for this process.
                    let lfd = LOG_FD.load(Ordering::SeqCst);
                    if lfd >= 0 {
                        libc::dup2(lfd, libc::STDOUT_FILENO);
                        libc::dup2(lfd, libc::STDERR_FILENO);
                    }
                    let pid_str = std::fmt::format(format_args!("{}", my_pid));
                    let mut pid_c = pid_str.into_bytes();
                    pid_c.push(0);
                    libc::execl(
                        b"/system/bin/logcat\0".as_ptr() as *const libc::c_char,
                        b"logcat\0".as_ptr() as *const libc::c_char,
                        b"-d\0".as_ptr() as *const libc::c_char,
                        b"--pid\0".as_ptr() as *const libc::c_char,
                        pid_c.as_ptr() as *const libc::c_char,
                        std::ptr::null::<libc::c_char>(),
                    );
                    libc::_exit(127);
                } else if child > 0 {
                    let mut status: libc::c_int = 0;
                    libc::waitpid(child, &mut status, 0);
                }
            }
            // Restore the default disposition and re-raise so the platform's
            // crash reporter (Android tombstone, core dumps) still sees it.
            let mut dfl: libc::sigaction = std::mem::zeroed();
            dfl.sa_sigaction = libc::SIG_DFL;
            libc::sigaction(sig, &dfl, std::ptr::null_mut());
            libc::raise(sig);
        }
    }

    /// Alternate signal stack. Without this, a host stack overflow (SIGSEGV
    /// on the guard page) kills the process silently: the handler itself
    /// faults because it has no stack to run on.
    static ALT_STACK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Install the diagnostic handlers for the fatal native signals.
    pub fn install() {
        resolve_backtrace();
        // Set up the alternate stack for the handler.
        const ALT_STACK_SIZE: usize = 1024 * 64;
        unsafe {
            let buf = libc::malloc(ALT_STACK_SIZE);
            if !buf.is_null() {
                ALT_STACK.store(buf as usize, Ordering::SeqCst);
                let ss = libc::stack_t {
                    ss_sp: buf,
                    ss_flags: 0,
                    ss_size: ALT_STACK_SIZE,
                };
                libc::sigaltstack(&ss, std::ptr::null_mut());
            }
        }
        let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
        act.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER | libc::SA_ONSTACK;
        act.sa_sigaction = handler as usize;
        for &sig in &[libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGABRT, libc::SIGFPE] {
            unsafe {
                libc::sigaction(sig, &act, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(unix)]
pub use imp::{install, native_backtrace_lines, set_log_fd};

#[cfg(not(unix))]
pub fn set_log_fd(_fd: i32) {}

#[cfg(not(unix))]
pub fn install() {}
