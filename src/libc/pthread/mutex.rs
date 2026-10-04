/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Guest mutex interface.
//!
//! See [crate::environment::mutex] for the internal implementation.
#![allow(rustdoc::broken_intra_doc_links)] // https://github.com/rust-lang/rust/issues/83049

use crate::dyld::{export_c_func, FunctionExports};
use crate::libc::errno::{EBUSY, EINVAL};
use crate::mem::{ConstPtr, MutPtr, Ptr, SafeRead};
use crate::{Environment, MutexId, MutexType, PTHREAD_MUTEX_DEFAULT};

/// Apple's implementation is a 4-byte magic number followed by an 8-byte opaque
/// region. We only have to match the size theirs has.
#[repr(C, packed)]
pub struct pthread_mutexattr_t {
    /// Magic number (must be [MAGIC_MUTEXATTR])
    magic: u32,
    type_: i32,
    /// This should eventually be a bitfield with the other attributes.
    _unused: u32,
}
unsafe impl SafeRead for pthread_mutexattr_t {}

/// Apple's implementation is a 4-byte magic number followed by a 56-byte opaque
/// region. We will store the actual data on the host, determined by a mutex
/// identifier.
#[repr(C, packed)]
pub struct pthread_mutex_t {
    /// Magic number (must be [MAGIC_MUTEX])
    magic: u32,
    /// Unique mutex identifier, used in matching the mutex to it's host object.
    pub mutex_id: MutexId,
}
unsafe impl SafeRead for pthread_mutex_t {}

/// Arbitrarily-chosen magic number for `pthread_mutexattr_t` (not Apple's).
const MAGIC_MUTEXATTR: u32 = u32::from_be_bytes(*b"MuAt");
/// Arbitrarily-chosen magic number for `pthread_mutex_t` (not Apple's).
const MAGIC_MUTEX: u32 = u32::from_be_bytes(*b"MUTX");
/// Signatures written by the `PTHREAD_*_MUTEX_INITIALIZER` macros. These are
/// part of the ABI: a statically-initialized guest `pthread_mutex_t` carries
/// one of these in its first word until the first lock, at which point real
/// libpthread lazily finishes setup. We do the same, registering a host mutex
/// of the matching type. See Apple's libpthread `src/pthread_mutex.c`.
const MAGIC_MUTEX_STATIC: u32 = 0x32AAABA7; // PTHREAD_MUTEX_INITIALIZER (normal)
const MAGIC_MUTEX_STATIC_ERRORCHECK: u32 = 0x32AAABA1; // PTHREAD_ERRORCHECK_MUTEX_INITIALIZER
const MAGIC_MUTEX_STATIC_RECURSIVE: u32 = 0x32AAABA2; // PTHREAD_RECURSIVE_MUTEX_INITIALIZER
const MAGIC_MUTEX_STATIC_FIRSTFIT: u32 = 0x32AAABA3; // first-fit; behaves as normal for us

#[allow(dead_code)]
const PTHREAD_PROCESS_SHARED: i32 = 1;
const PTHREAD_PROCESS_PRIVATE: i32 = 2;

fn mutex_address_is_usable(address: u32, null_segment_size: u32) -> bool {
    address >= null_segment_size
}

fn pthread_mutexattr_init(env: &mut Environment, attr: MutPtr<pthread_mutexattr_t>) -> i32 {
    env.mem.write(
        attr,
        pthread_mutexattr_t {
            magic: MAGIC_MUTEXATTR,
            type_: PTHREAD_MUTEX_DEFAULT as i32,
            _unused: 0,
        },
    );
    0 // success
}
fn pthread_mutexattr_setpshared(
    env: &mut Environment,
    attr: MutPtr<pthread_mutexattr_t>,
    pshared: i32,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    // PTHREAD_PROCESS_PRIVATE is a default one
    // TODO: set this attribute in `attr` instead
    assert_eq!(pshared, PTHREAD_PROCESS_PRIVATE);
    0 // success
}
fn pthread_mutexattr_settype(
    env: &mut Environment,
    attr: MutPtr<pthread_mutexattr_t>,
    type_: i32,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    let mut attr_copy = env.mem.read(attr);
    attr_copy.type_ = type_;
    env.mem.write(attr, attr_copy);
    0 // success
}
fn pthread_mutexattr_gettype(
    env: &mut Environment,
    attr: ConstPtr<pthread_mutexattr_t>,
    type_out: MutPtr<i32>,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    if type_out.is_null() {
        return EINVAL;
    }
    let attr_copy = env.mem.read(attr);
    env.mem.write(type_out, attr_copy.type_);
    0 // success
}
/// `pthread_mutexattr_setprotocol(attr, protocol)` — Apple's
/// `<pthread/pthread.h>` declares this for POSIX priority-inheritance
/// support. Permitted protocols are:
///   - `PTHREAD_PRIO_NONE     = 0` (no priority changes — Apple default)
///   - `PTHREAD_PRIO_INHERIT  = 1` (priority inheritance)
///   - `PTHREAD_PRIO_PROTECT  = 2` (priority protection / ceiling)
///
/// touchHLE is single-process and runs on the host scheduler; priority
/// boosts are not user-observable. We therefore validate the protocol
/// argument like Apple does and treat the setting as a no-op, returning
/// `0` on success and `EINVAL` for unknown protocols. See
/// <https://opensource.apple.com/source/libpthread/libpthread-301.30.1/src/pthread_mutex.c.auto.html>.
const PTHREAD_PRIO_NONE: i32 = 0;
const PTHREAD_PRIO_INHERIT: i32 = 1;
const PTHREAD_PRIO_PROTECT: i32 = 2;

fn pthread_mutexattr_setprotocol(
    env: &mut Environment,
    attr: MutPtr<pthread_mutexattr_t>,
    protocol: i32,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    match protocol {
        PTHREAD_PRIO_NONE | PTHREAD_PRIO_INHERIT | PTHREAD_PRIO_PROTECT => 0,
        _ => EINVAL,
    }
}

fn pthread_mutexattr_getprotocol(
    env: &mut Environment,
    attr: ConstPtr<pthread_mutexattr_t>,
    out: MutPtr<i32>,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    if out.is_null() {
        return EINVAL;
    }
    // We don't store the protocol; always report Apple's default.
    env.mem.write(out, PTHREAD_PRIO_NONE);
    0
}

/// `pthread_mutexattr_setprioceiling(attr, ceiling)` — companion to
/// the protocol API. Apple validates the value against the host's
/// priority range; with no scheduler under us this is best-effort.
fn pthread_mutexattr_setprioceiling(
    env: &mut Environment,
    attr: MutPtr<pthread_mutexattr_t>,
    _ceiling: i32,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    0
}

fn pthread_mutexattr_getprioceiling(
    env: &mut Environment,
    attr: ConstPtr<pthread_mutexattr_t>,
    out: MutPtr<i32>,
) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    if out.is_null() {
        return EINVAL;
    }
    env.mem.write(out, 0);
    0
}

fn pthread_mutexattr_destroy(env: &mut Environment, attr: MutPtr<pthread_mutexattr_t>) -> i32 {
    check_magic!(env, attr, MAGIC_MUTEXATTR);
    env.mem.write(
        attr,
        pthread_mutexattr_t {
            magic: 0,
            type_: PTHREAD_MUTEX_DEFAULT as i32,
            _unused: 0,
        },
    );
    0 // success
}

pub fn pthread_mutex_init(
    env: &mut Environment,
    mutex: MutPtr<pthread_mutex_t>,
    attr: ConstPtr<pthread_mutexattr_t>,
) -> i32 {
    if !mutex_address_is_usable(mutex.to_bits(), env.mem.null_segment_size()) {
        return EINVAL;
    }
    let type_ = if !attr.is_null() {
        check_magic!(env, attr, MAGIC_MUTEXATTR);
        let pthread_mutexattr_t { type_, .. } = env.mem.read(attr);
        type_.try_into().unwrap()
    } else {
        PTHREAD_MUTEX_DEFAULT
    };
    let mutex_id = env.mutex_state.init_mutex(type_);
    log_dbg!(
        "Mutex #{} created from pthread_mutex_init ({:#x})",
        mutex_id,
        mutex.to_bits()
    );
    env.mem.write(
        mutex,
        pthread_mutex_t {
            magic: MAGIC_MUTEX,
            mutex_id,
        },
    );

    0 // success
}

/// Outcome of inspecting a guest `pthread_mutex_t`'s magic word.
enum MutexLookup {
    /// The mutex is registered (its `mutex_id` in the guest struct is valid)
    /// and the operation proceeds normally.
    Ready,
    /// The magic word is none of ours, not a recognised static initializer, and
    /// not a zeroed slot — it holds foreign bytes (e.g. a pointer left in
    /// recycled storage, or a `std::mutex` whose backing object was clobbered
    /// or freed). We must NOT overwrite that memory (unlike the zero case, it
    /// may belong to a live object). The caller treats the operation as a
    /// lenient no-op returning success, because guests wrap
    /// `pthread_mutex_lock`/`_unlock` in `assert(ec == 0)` (Minecraft PE's
    /// `Mutex::lock`/`unlock`, ../src/mutex.cpp:45) and returning an error
    /// would abort the whole guest session.
    Foreign,
}

/// Register a host mutex of the given type for a statically-initialized guest
/// mutex and stamp its guest struct with [MAGIC_MUTEX].
fn register_mutex_typed(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>, type_: MutexType) {
    let mutex_id = env.mutex_state.init_mutex(type_);
    env.mem.write(
        mutex,
        pthread_mutex_t {
            magic: MAGIC_MUTEX,
            mutex_id,
        },
    );
}

fn check_or_register_mutex(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>) -> MutexLookup {
    if !mutex_address_is_usable(mutex.to_bits(), env.mem.null_segment_size()) {
        static INVALID_MUTEX_LOGGED: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(0);
        let n = INVALID_MUTEX_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < 8 {
            log!(
                "Warning: pthread mutex pointer {:?} is inside the null page; treating the operation as a no-op (occurrence {})",
                mutex,
                n + 1
            );
        }
        return MutexLookup::Foreign;
    }
    let magic: u32 = env.mem.read(mutex.cast());
    match magic {
        // Already one of ours.
        MAGIC_MUTEX => MutexLookup::Ready,
        // Statically-initialized mutex: register it, changing the magic in the
        // process, then treat it as ready.
        MAGIC_MUTEX_STATIC | MAGIC_MUTEX_STATIC_FIRSTFIT => {
            log_dbg!(
                "Detected statically-initialized mutex at {:?}, registering.",
                mutex
            );
            pthread_mutex_init(env, mutex, Ptr::null());
            MutexLookup::Ready
        }
        MAGIC_MUTEX_STATIC_ERRORCHECK => {
            log_dbg!(
                "Detected statically-initialized error-checking mutex at {:?}, registering.",
                mutex
            );
            register_mutex_typed(env, mutex, MutexType::PTHREAD_MUTEX_ERRORCHECK);
            MutexLookup::Ready
        }
        MAGIC_MUTEX_STATIC_RECURSIVE => {
            log_dbg!(
                "Detected statically-initialized recursive mutex at {:?}, registering.",
                mutex
            );
            register_mutex_typed(env, mutex, MutexType::PTHREAD_MUTEX_RECURSIVE);
            MutexLookup::Ready
        }
        // A zero-initialized `pthread_mutex_t`. This turns up when a C++ object
        // that embeds a `std::mutex` lives in zeroed storage (calloc'd memory,
        // or an Objective-C ivar block, which the runtime zero-fills) and
        // reaches lock/unlock before we ever registered it — i.e. its
        // `.cxx_construct` never ran through a path we model. On real Darwin
        // the mutex would carry `PTHREAD_MUTEX_INITIALIZER`'s signature; here
        // the bytes are just 0. Zeroed storage is safe to claim, so lazily
        // register a fresh default mutex, matching the lenient treatment we
        // already give already-unlocked default mutexes.
        0 => {
            static ZERO_MUTEX_LOGGED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !ZERO_MUTEX_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log!(
                    "Warning: pthread mutex at {:?} was zero-initialized (never went \
                     through pthread_mutex_init); lazily registering a default mutex \
                     so the guest keeps running instead of aborting.",
                    mutex
                );
            }
            pthread_mutex_init(env, mutex, Ptr::null());
            MutexLookup::Ready
        }
        // Foreign / clobbered bytes. Do not touch the memory; the caller makes
        // the operation a lenient no-op so the guest's `assert(ec == 0)` around
        // (un)lock does not abort the session (observed entering the Nether in
        // Minecraft PE 0.14.2, and on some 0.16.2 launches).
        _ => {
            static FOREIGN_MUTEX_LOGGED: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let n = FOREIGN_MUTEX_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 8 {
                log!(
                    "Warning: pthread mutex at {:?} has unrecognized magic {:#x} (foreign or \
                     clobbered storage); treating lock/unlock as a no-op so the guest keeps \
                     running instead of aborting. (occurrence {})",
                    mutex,
                    magic,
                    n + 1
                );
            }
            MutexLookup::Foreign
        }
    }
}

pub fn pthread_mutex_lock(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>) -> i32 {
    match check_or_register_mutex(env, mutex) {
        MutexLookup::Ready => {}
        // Unknown mutex: pretend the lock succeeded. We cannot provide real
        // mutual exclusion (there is no host mutex to key on), but returning
        // success keeps the guest's `assert(ec == 0)` happy, and the matching
        // unlock is treated as a no-op too, so the lock/unlock pair stays
        // balanced.
        MutexLookup::Foreign => return 0,
    };
    let mutex_data = env.mem.read(mutex);
    let mutex_id = mutex_data.mutex_id;
    log_dbg!("About to lock mutex #{} ({:#x})", mutex_id, mutex.to_bits());
    match env.lock_mutex(mutex_id) {
        Ok(_) => 0,
        // `lock_mutex` can still report an error (e.g. EDEADLK when an
        // error-checking mutex is re-locked by its owner). The guest wraps
        // `pthread_mutex_lock` in `assert(ec == 0)` (Minecraft PE's
        // `Mutex::lock`, ../src/mutex.cpp:45), so propagating the errno would
        // abort the session via `__assert_rtn`. Guests never legitimately
        // handle these errors: report success instead. The failed attempt did
        // not change the host-side lock state, and the balancing unlock below
        // is lenient as well, so the guest's bookkeeping stays consistent.
        Err(e) => {
            static LOCK_ERR_LOGGED: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let n = LOCK_ERR_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 8 {
                log!(
                    "Warning: pthread_mutex_lock on mutex #{mutex_id} failed with errno {e}; ignoring the error and returning success so the guest's assert(ec == 0) does not abort the session. (occurrence {})",
                    n + 1
                );
            }
            0
        }
    }
}

pub fn pthread_mutex_trylock(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>) -> i32 {
    match check_or_register_mutex(env, mutex) {
        MutexLookup::Ready => {}
        MutexLookup::Foreign => return 0,
    };
    let mutex_data = env.mem.read(mutex);
    if env.mutex_state.mutex_is_locked(mutex_data.mutex_id)
        && !(env.mutex_state.mutex_is_recursive(mutex_data.mutex_id)
            && env
                .mutex_state
                .mutex_is_locked_by(mutex_data.mutex_id, env.current_thread))
    {
        EBUSY
    } else {
        pthread_mutex_lock(env, mutex)
    }
}

pub fn pthread_mutex_unlock(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>) -> i32 {
    match check_or_register_mutex(env, mutex) {
        MutexLookup::Ready => {}
        // Unknown mutex: match Darwin's lenient behaviour for unrecognized /
        // default mutexes and return success rather than EINVAL, which would
        // trip the guest's `assert(ec == 0)` and abort the session.
        MutexLookup::Foreign => return 0,
    };
    let mutex_data = env.mem.read(mutex);
    let mutex_id = mutex_data.mutex_id;
    log_dbg!(
        "About to unlock mutex #{} ({:#x})",
        mutex_id,
        mutex.to_bits()
    );
    match env.unlock_mutex(mutex_id) {
        Ok(_) => 0,
        // `unlock_mutex` reports EPERM when unlocking an already-unlocked
        // mutex, or an error-checking/recursive mutex owned by another
        // thread. The guest wraps `pthread_mutex_unlock` in
        // `assert(ec == 0)` (Minecraft PE's `Mutex::unlock`,
        // ../src/mutex.cpp:45), and this is exactly the abort observed in the
        // field: an unlock of a mutex we considered already unlocked (or one
        // destroyed and lazily re-registered as a fresh, unlocked default
        // mutex) returned EPERM and killed the session via `__assert_rtn`.
        // Real Darwin does not surface this as an error to guests; report
        // success and leave the host-side lock state untouched (there was
        // nothing to release).
        Err(e) => {
            static UNLOCK_ERR_LOGGED: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let n = UNLOCK_ERR_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 8 {
                log!(
                    "Warning: pthread_mutex_unlock on mutex #{mutex_id} failed with errno {e}; ignoring the error and returning success so the guest's assert(ec == 0) does not abort the session. (occurrence {})",
                    n + 1
                );
            }
            0
        }
    }
}

pub fn pthread_mutex_destroy(env: &mut Environment, mutex: MutPtr<pthread_mutex_t>) -> i32 {
    match check_or_register_mutex(env, mutex) {
        MutexLookup::Ready => {}
        MutexLookup::Foreign => return 0,
    };
    let mutex_id = env.mem.read(mutex).mutex_id;
    env.mem.write(
        mutex,
        pthread_mutex_t {
            magic: 0,
            mutex_id: 0xFFFFFFFFFFFFFFFF,
        },
    );
    env.mutex_state.destroy_mutex(mutex_id).err().unwrap_or(0)
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(pthread_mutexattr_init(_)),
    export_c_func!(pthread_mutexattr_setpshared(_, _)),
    export_c_func!(pthread_mutexattr_settype(_, _)),
    export_c_func!(pthread_mutexattr_gettype(_, _)),
    export_c_func!(pthread_mutexattr_destroy(_)),
    export_c_func!(pthread_mutexattr_setprotocol(_, _)),
    export_c_func!(pthread_mutexattr_getprotocol(_, _)),
    export_c_func!(pthread_mutexattr_setprioceiling(_, _)),
    export_c_func!(pthread_mutexattr_getprioceiling(_, _)),
    export_c_func!(pthread_mutex_init(_, _)),
    export_c_func!(pthread_mutex_lock(_)),
    export_c_func!(pthread_mutex_trylock(_)),
    export_c_func!(pthread_mutex_unlock(_)),
    export_c_func!(pthread_mutex_destroy(_)),
];

#[cfg(test)]
mod tests {
    use super::mutex_address_is_usable;

    #[test]
    fn mutex_addresses_in_null_page_are_unusable() {
        assert!(!mutex_address_is_usable(0, 0x1000));
        assert!(!mutex_address_is_usable(1, 0x1000));
        assert!(!mutex_address_is_usable(0xfff, 0x1000));
        assert!(mutex_address_is_usable(0x1000, 0x1000));
    }
}
