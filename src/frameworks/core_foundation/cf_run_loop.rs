/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFRunLoop`.
//!
//! This is not even toll-free bridged to `NSRunLoop` in Apple's implementation,
//! but here it is the same type.

use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_foundation::time::CFTimeInterval;
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::foundation::ns_run_loop::run_run_loop_single_iteration;
use crate::frameworks::foundation::ns_string;
use crate::mem::{SafeRead, GuestISize, ConstPtr, MutVoidPtr};
use crate::objc::{id, msg, msg_class, nil, HostObject};
use crate::Environment;

pub type CFRunLoopRef     = CFTypeRef;
pub type CFRunLoopMode    = super::cf_string::CFStringRef;
pub type CFRunLoopSourceRef = CFTypeRef;
pub type CFRunLoopObserverRef = CFTypeRef;
pub type CFRunLoopTimerRef  = CFTypeRef;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CFRunLoopTimerContext {
    pub version: GuestISize, // ИСПОРАВЛЕНО: GuestISize гарантирует 4 байта, как в iOS
    pub info: MutVoidPtr,
    pub retain: GuestFunction,
    pub release: GuestFunction,
    pub copyDescription: GuestFunction,
}

unsafe impl SafeRead for CFRunLoopTimerContext {}

// Наш внутренний объект для хранения таймера в памяти эмулятора
pub struct CFRunLoopTimerHostObject {
    pub fire_date: f64,
    pub interval: f64,
    pub flags: u32,
    pub order: i32,
    pub context: CFRunLoopTimerContext,
    pub callout: GuestFunction,
    pub is_valid: bool,
}

impl HostObject for CFRunLoopTimerHostObject {}

// CFRunLoopRunResult
const kCFRunLoopRunFinished:     i32 = 1;
const kCFRunLoopRunStopped:      i32 = 2;
const kCFRunLoopRunTimedOut:     i32 = 3;
const kCFRunLoopRunHandledSource: i32 = 4;

// CFRunLoopActivity flags
type CFRunLoopActivity = u32;
const kCFRunLoopEntry:         CFRunLoopActivity = 1 << 0;
const kCFRunLoopBeforeTimers:  CFRunLoopActivity = 1 << 1;
const kCFRunLoopBeforeSources: CFRunLoopActivity = 1 << 2;
const kCFRunLoopBeforeWaiting: CFRunLoopActivity = 1 << 5;
const kCFRunLoopAfterWaiting:  CFRunLoopActivity = 1 << 6;
const kCFRunLoopExit:          CFRunLoopActivity = 1 << 7;
const kCFRunLoopAllActivities: CFRunLoopActivity = 0x0FFFFFFF;

pub const kCFRunLoopCommonModes: &str = "kCFRunLoopCommonModes";
pub const kCFRunLoopDefaultMode: &str = "kCFRunLoopDefaultMode";

pub const CONSTANTS: ConstantExports = &[
    ("_kCFRunLoopCommonModes", HostConstant::NSString(kCFRunLoopCommonModes)),
    ("_kCFRunLoopDefaultMode", HostConstant::NSString(kCFRunLoopDefaultMode)),
];

// MARK: - Helpers

fn is_known_mode(env: &mut Environment, mode: CFRunLoopMode) -> bool {
    let default_mode = ns_string::get_static_str(env, kCFRunLoopDefaultMode);
    let common_modes = ns_string::get_static_str(env, kCFRunLoopCommonModes);
    msg![env; mode isEqualToString:default_mode]
        || msg![env; mode isEqualToString:common_modes]
}

// MARK: - Retain / Release

pub fn CFRunLoopRetain(env: &mut Environment, rl: CFRunLoopRef) -> CFRunLoopRef {
    if !rl.is_null() { CFRetain(env, rl) } else { rl }
}

pub fn CFRunLoopRelease(env: &mut Environment, rl: CFRunLoopRef) {
    if !rl.is_null() { CFRelease(env, rl); }
}

// MARK: - Accessors

fn CFRunLoopGetCurrent(env: &mut Environment) -> CFRunLoopRef {
    msg_class![env; NSRunLoop currentRunLoop]
}

pub fn CFRunLoopGetMain(env: &mut Environment) -> CFRunLoopRef {
    msg_class![env; NSRunLoop mainRunLoop]
}

fn CFRunLoopCopyCurrentMode(env: &mut Environment, _rl: CFRunLoopRef) -> CFRunLoopMode {
    // We only support the default mode.
    let s = ns_string::from_rust_string(env, kCFRunLoopDefaultMode.to_string());
    crate::objc::autorelease(env, s)
}

fn CFRunLoopCopyAllModes(env: &mut Environment, _rl: CFRunLoopRef) -> CFTypeRef {
    // Return an array containing only the default mode.
    let mode = ns_string::get_static_str(env, kCFRunLoopDefaultMode);
    let arr: id = msg_class![env; NSArray arrayWithObject:mode];
    arr
}

fn CFRunLoopContainsSource(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _source: CFRunLoopSourceRef,
    _mode: CFRunLoopMode,
) -> bool {
    false
}

fn CFRunLoopContainsObserver(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _observer: CFRunLoopObserverRef,
    _mode: CFRunLoopMode,
) -> bool {
    false
}

fn CFRunLoopContainsTimer(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _timer: CFRunLoopTimerRef,
    _mode: CFRunLoopMode,
) -> bool {
    false
}

fn CFRunLoopIsWaiting(_env: &mut Environment, _rl: CFRunLoopRef) -> bool {
    false
}

// MARK: - Run

fn CFRunLoopRun(env: &mut Environment) {
    let rl = CFRunLoopGetCurrent(env);
    let _: () = msg![env; rl run];
}

fn CFRunLoopRunInMode(
    env: &mut Environment,
    mode: CFRunLoopMode,
    seconds: CFTimeInterval,
    _return_after_source_handled: bool,
) -> i32 {
    if !is_known_mode(env, mode) {
        log!("CFRunLoopRunInMode: unknown mode, skipping");
        return kCFRunLoopRunFinished;
    }
    let current = CFRunLoopGetCurrent(env);
    if seconds == 0.0 {
        run_run_loop_single_iteration(env, current);
    } else {
        let limit: id = msg_class![env; NSDate dateWithTimeIntervalSinceNow:seconds];
        let _: () = msg![env; current runUntilDate:limit];
    }
    kCFRunLoopRunFinished
}

fn CFRunLoopStop(_env: &mut Environment, _rl: CFRunLoopRef) {
    log!("CFRunLoopStop: stubbed (run loop cannot be stopped externally)");
}

fn CFRunLoopWakeUp(_env: &mut Environment, _rl: CFRunLoopRef) {
    log_dbg!("CFRunLoopWakeUp: stubbed");
}

// MARK: - Sources

fn CFRunLoopAddSource(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _source: CFRunLoopSourceRef,
    _mode: CFRunLoopMode,
) {
    log!("CFRunLoopAddSource: stubbed");
}

fn CFRunLoopRemoveSource(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _source: CFRunLoopSourceRef,
    _mode: CFRunLoopMode,
) {
    log!("CFRunLoopRemoveSource: stubbed");
}

fn CFRunLoopSourceCreate(
    _env: &mut Environment,
    _allocator: CFTypeRef,
    _order: i32,
    _context: MutVoidPtr, // CFRunLoopSourceContext*
) -> CFRunLoopSourceRef {
    log!("CFRunLoopSourceCreate: stubbed, returning null");
    nil
}

fn CFRunLoopSourceRetain(env: &mut Environment, source: CFRunLoopSourceRef) -> CFRunLoopSourceRef {
    if !source.is_null() { CFRetain(env, source) } else { source }
}

fn CFRunLoopSourceRelease(env: &mut Environment, source: CFRunLoopSourceRef) {
    if !source.is_null() { CFRelease(env, source); }
}

fn CFRunLoopSourceSignal(_env: &mut Environment, _source: CFRunLoopSourceRef) {
    log_dbg!("CFRunLoopSourceSignal: stubbed");
}

fn CFRunLoopSourceIsValid(_env: &mut Environment, source: CFRunLoopSourceRef) -> bool {
    !source.is_null()
}

fn CFRunLoopSourceInvalidate(_env: &mut Environment, _source: CFRunLoopSourceRef) {
    log_dbg!("CFRunLoopSourceInvalidate: stubbed");
}

// MARK: - Observers

fn CFRunLoopAddObserver(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _observer: CFRunLoopObserverRef,
    _mode: CFRunLoopMode,
) {
    log!("CFRunLoopAddObserver: stubbed");
}

fn CFRunLoopRemoveObserver(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _observer: CFRunLoopObserverRef,
    _mode: CFRunLoopMode,
) {
    log!("CFRunLoopRemoveObserver: stubbed");
}

fn CFRunLoopObserverCreate(
    _env: &mut Environment,
    _allocator: CFTypeRef,
    _activities: CFRunLoopActivity,
    _repeats: bool,
    _order: i32,
    _callout: MutVoidPtr,  // CFRunLoopObserverCallBack
    _context: MutVoidPtr,  // CFRunLoopObserverContext*
) -> CFRunLoopObserverRef {
    log!("CFRunLoopObserverCreate: stubbed, returning null");
    nil
}

fn CFRunLoopObserverRetain(
    env: &mut Environment,
    observer: CFRunLoopObserverRef,
) -> CFRunLoopObserverRef {
    if !observer.is_null() { CFRetain(env, observer) } else { observer }
}

fn CFRunLoopObserverRelease(env: &mut Environment, observer: CFRunLoopObserverRef) {
    if !observer.is_null() { CFRelease(env, observer); }
}

fn CFRunLoopObserverIsValid(_env: &mut Environment, observer: CFRunLoopObserverRef) -> bool {
    !observer.is_null()
}

fn CFRunLoopObserverInvalidate(_env: &mut Environment, _observer: CFRunLoopObserverRef) {
    log_dbg!("CFRunLoopObserverInvalidate: stubbed");
}

fn CFRunLoopObserverGetActivities(
    _env: &mut Environment,
    _observer: CFRunLoopObserverRef,
) -> CFRunLoopActivity {
    kCFRunLoopAllActivities
}

fn CFRunLoopObserverDoesRepeat(
    _env: &mut Environment,
    _observer: CFRunLoopObserverRef,
) -> bool {
    false
}

fn CFRunLoopObserverGetOrder(_env: &mut Environment, _observer: CFRunLoopObserverRef) -> i32 {
    0
}

// MARK: - Timers

fn CFRunLoopAddTimer(
    env: &mut Environment,
    rl: CFRunLoopRef,
    timer: CFRunLoopTimerRef,
    mode: CFRunLoopMode,
) {
    if rl.is_null() || timer.is_null() {
        return;
    }

    // Как сказано в заголовке файла: в HyperHLE CFRunLoop и NSRunLoop — это один и тот же тип.
    // Поэтому мы честно пробрасываем вызов напрямую в NSRunLoop, который умеет работать с таймерами.
    let _: () = msg![env; rl addTimer:timer forMode:mode];
}

fn CFRunLoopRemoveTimer(
    _env: &mut Environment,
    _rl: CFRunLoopRef,
    _timer: CFRunLoopTimerRef,
    _mode: CFRunLoopMode,
) {
    log!("CFRunLoopRemoveTimer: stubbed");
}

fn CFRunLoopTimerCreate(
    env: &mut Environment,
    _allocator: id,
    fire_date: f64,
    interval: f64,
    flags: u32,
    order: i32,
    context_ptr: ConstPtr<CFRunLoopTimerContext>,
    callout: GuestFunction,
) -> CFRunLoopTimerRef {
    // 1. Честно считываем контекст из памяти гостя, если он передан
    let context = if !context_ptr.is_null() {
        env.mem.read(context_ptr)
    } else {
        // Если игра передала NULL, заполняем структуру нулями
        unsafe { std::mem::zeroed() }
    };

    // 2. Упаковываем все данные в наш HostObject
    let host_object = CFRunLoopTimerHostObject {
        fire_date,
        interval,
        flags,
        order,
        context,
        callout,
        is_valid: true, // Таймер активен при создании
    };

    // 3. Выделяем реальный объект, чтобы игра не получила null.
    // Используем базовый класс NSObject (или если в HyperHLE есть NSTimer, то его)
    let class = env.objc.get_known_class("NSObject", &mut env.mem);
    
    // Возвращаем настоящий валидный указатель на созданный объект
    env.objc.alloc_object(class, Box::new(host_object), &mut env.mem)
}

fn CFRunLoopTimerRetain(
    env: &mut Environment,
    timer: CFRunLoopTimerRef,
) -> CFRunLoopTimerRef {
    if !timer.is_null() { CFRetain(env, timer) } else { timer }
}

fn CFRunLoopTimerRelease(env: &mut Environment, timer: CFRunLoopTimerRef) {
    if !timer.is_null() { CFRelease(env, timer); }
}

fn CFRunLoopTimerIsValid(_env: &mut Environment, timer: CFRunLoopTimerRef) -> bool {
    !timer.is_null()
}

fn CFRunLoopTimerInvalidate(_env: &mut Environment, _timer: CFRunLoopTimerRef) {
    log_dbg!("CFRunLoopTimerInvalidate: stubbed");
}

fn CFRunLoopTimerGetNextFireDate(
    _env: &mut Environment,
    _timer: CFRunLoopTimerRef,
) -> CFTimeInterval {
    0.0
}

fn CFRunLoopTimerSetNextFireDate(
    _env: &mut Environment,
    _timer: CFRunLoopTimerRef,
    _fire_date: CFTimeInterval,
) {
    log_dbg!("CFRunLoopTimerSetNextFireDate: stubbed");
}

fn CFRunLoopTimerGetInterval(
    _env: &mut Environment,
    _timer: CFRunLoopTimerRef,
) -> CFTimeInterval {
    0.0
}

fn CFRunLoopTimerDoesRepeat(_env: &mut Environment, _timer: CFRunLoopTimerRef) -> bool {
    false
}

fn CFRunLoopTimerGetOrder(_env: &mut Environment, _timer: CFRunLoopTimerRef) -> i32 {
    0
}

pub const FUNCTIONS: FunctionExports = &[
    // Run loop lifecycle
    export_c_func!(CFRunLoopRetain(_)),
    export_c_func!(CFRunLoopRelease(_)),
    export_c_func!(CFRunLoopGetCurrent()),
    export_c_func!(CFRunLoopGetMain()),
    export_c_func!(CFRunLoopCopyCurrentMode(_)),
    export_c_func!(CFRunLoopCopyAllModes(_)),
    export_c_func!(CFRunLoopIsWaiting(_)),
    export_c_func!(CFRunLoopContainsSource(_, _, _)),
    export_c_func!(CFRunLoopContainsObserver(_, _, _)),
    export_c_func!(CFRunLoopContainsTimer(_, _, _)),
    // Running
    export_c_func!(CFRunLoopRun()),
    export_c_func!(CFRunLoopRunInMode(_, _, _)),
    export_c_func!(CFRunLoopStop(_)),
    export_c_func!(CFRunLoopWakeUp(_)),
    // Sources
    export_c_func!(CFRunLoopAddSource(_, _, _)),
    export_c_func!(CFRunLoopRemoveSource(_, _, _)),
    export_c_func!(CFRunLoopSourceCreate(_, _, _)),
    export_c_func!(CFRunLoopSourceRetain(_)),
    export_c_func!(CFRunLoopSourceRelease(_)),
    export_c_func!(CFRunLoopSourceSignal(_)),
    export_c_func!(CFRunLoopSourceIsValid(_)),
    export_c_func!(CFRunLoopSourceInvalidate(_)),
    // Observers
    export_c_func!(CFRunLoopAddObserver(_, _, _)),
    export_c_func!(CFRunLoopRemoveObserver(_, _, _)),
    export_c_func!(CFRunLoopObserverCreate(_, _, _, _, _, _)),
    export_c_func!(CFRunLoopObserverRetain(_)),
    export_c_func!(CFRunLoopObserverRelease(_)),
    export_c_func!(CFRunLoopObserverIsValid(_)),
    export_c_func!(CFRunLoopObserverInvalidate(_)),
    export_c_func!(CFRunLoopObserverGetActivities(_)),
    export_c_func!(CFRunLoopObserverDoesRepeat(_)),
    export_c_func!(CFRunLoopObserverGetOrder(_)),
    // Timers
    export_c_func!(CFRunLoopAddTimer(_, _, _)),
    export_c_func!(CFRunLoopRemoveTimer(_, _, _)),
    export_c_func!(CFRunLoopTimerCreate(_, _, _, _, _, _, _)),
    export_c_func!(CFRunLoopTimerRetain(_)),
    export_c_func!(CFRunLoopTimerRelease(_)),
    export_c_func!(CFRunLoopTimerIsValid(_)),
    export_c_func!(CFRunLoopTimerInvalidate(_)),
    export_c_func!(CFRunLoopTimerGetNextFireDate(_)),
    export_c_func!(CFRunLoopTimerSetNextFireDate(_, _)),
    export_c_func!(CFRunLoopTimerGetInterval(_)),
    export_c_func!(CFRunLoopTimerDoesRepeat(_)),
    export_c_func!(CFRunLoopTimerGetOrder(_)),
];
