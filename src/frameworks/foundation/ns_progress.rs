/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! `NSProgress` (iOS 7+). SDKs (notably Appodeal's download/asset code) query
//! and update progress objects during start-up; a missing class raised
//! `NSInvalidArgumentException` inside Unity's IL2CPP runtime, which has no
//! unwinder here and left the game looping on the same exception. The class is
//! bookkeeping only: touchHLE delivers network data synchronously, so there is
//! nothing to actually observe.
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
};

type NSInteger = i32;

#[derive(Default)]
struct NSProgressHostObject {
    total_unit_count: i64,
    completed_unit_count: i64,
    cancelled: bool,
    paused: bool,
    pausable: bool,
    cancellable: bool,
    indeterminate: bool,
    kind: id,
    localized_description: id,
    user_info: id,
    cancellation_handler: id,
    pausing_handler: id,
    resuming_handler: id,
}
impl HostObject for NSProgressHostObject {}

fn set_handler(
    env: &mut crate::Environment,
    object: id,
    handler: id,
    field: fn(&mut NSProgressHostObject) -> &mut id,
) {
    let copy: id = if handler == nil { nil } else { msg![env; handler copy] };
    let old = *field(env.objc.borrow_mut::<NSProgressHostObject>(object));
    *field(env.objc.borrow_mut::<NSProgressHostObject>(object)) = copy;
    release(env, old);
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSProgress: NSObject

+ (id)allocWithZone:(crate::objc::NSZonePtr)_zone {
    let host = Box::<NSProgressHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)progressWithTotalUnitCount:(i64)total {
    let progress: id = msg![env; this alloc];
    let progress: id = msg![env; progress initWithParent:nil userInfo:nil];
    () = msg![env; progress setTotalUnitCount:total];
    crate::objc::autorelease(env, progress)
}

- (id)initWithParent:(id)parent userInfo:(id)user_info {
    let this: id = crate::objc::msg_super![env; this init];
    retain(env, user_info);
    env.objc
        .borrow_mut::<NSProgressHostObject>(this)
        .user_info = user_info;
    this
}

- (i64)totalUnitCount {
    env.objc.borrow::<NSProgressHostObject>(this).total_unit_count
}
- (())setTotalUnitCount:(i64)value {
    env.objc
        .borrow_mut::<NSProgressHostObject>(this)
        .total_unit_count = value;
}
- (i64)completedUnitCount {
    env.objc
        .borrow::<NSProgressHostObject>(this)
        .completed_unit_count
}
- (())setCompletedUnitCount:(i64)value {
    env.objc
        .borrow_mut::<NSProgressHostObject>(this)
        .completed_unit_count = value;
}
- (f64)fractionCompleted {
    let host = env.objc.borrow::<NSProgressHostObject>(this);
    if host.total_unit_count <= 0 {
        0.0
    } else {
        (host.completed_unit_count as f64 / host.total_unit_count as f64).clamp(0.0, 1.0)
    }
}
- (bool)isIndeterminate {
    env.objc.borrow::<NSProgressHostObject>(this).indeterminate
}
- (bool)isCancelled {
    env.objc.borrow::<NSProgressHostObject>(this).cancelled
}
- (())cancel {
    env.objc.borrow_mut::<NSProgressHostObject>(this).cancelled = true;
}
- (bool)isPaused {
    env.objc.borrow::<NSProgressHostObject>(this).paused
}
- (bool)isPausable {
    env.objc.borrow::<NSProgressHostObject>(this).pausable
}
- (())setPausable:(bool)value {
    env.objc.borrow_mut::<NSProgressHostObject>(this).pausable = value;
}
- (())pause {
    env.objc.borrow_mut::<NSProgressHostObject>(this).paused = true;
}
- (())resume {
    env.objc.borrow_mut::<NSProgressHostObject>(this).paused = false;
}
- (bool)isCancellable {
    env.objc.borrow::<NSProgressHostObject>(this).cancellable
}
- (())setCancellable:(bool)value {
    env.objc.borrow_mut::<NSProgressHostObject>(this).cancellable = value;
}
- (id)cancellationHandler {
    env.objc.borrow::<NSProgressHostObject>(this).cancellation_handler
}
- (())setCancellationHandler:(id)handler {
    set_handler(env, this, handler, |host| &mut host.cancellation_handler);
}
- (id)pausingHandler {
    env.objc.borrow::<NSProgressHostObject>(this).pausing_handler
}
- (())setPausingHandler:(id)handler {
    set_handler(env, this, handler, |host| &mut host.pausing_handler);
}
- (id)resumingHandler {
    env.objc.borrow::<NSProgressHostObject>(this).resuming_handler
}
- (())setResumingHandler:(id)handler {
    set_handler(env, this, handler, |host| &mut host.resuming_handler);
}

- (id)kind {
    env.objc.borrow::<NSProgressHostObject>(this).kind
}
- (())setKind:(id)kind {
    let copy: id = if kind == nil { nil } else { msg![env; kind copy] };
    let old = env.objc.borrow::<NSProgressHostObject>(this).kind;
    env.objc.borrow_mut::<NSProgressHostObject>(this).kind = copy;
    release(env, old);
}

- (id)localizedDescription {
    env.objc
        .borrow::<NSProgressHostObject>(this)
        .localized_description
}
- (())setLocalizedDescription:(id)description {
    let copy: id = if description == nil {
        nil
    } else {
        msg![env; description copy]
    };
    let old = env
        .objc
        .borrow::<NSProgressHostObject>(this)
        .localized_description;
    env.objc
        .borrow_mut::<NSProgressHostObject>(this)
        .localized_description = copy;
    release(env, old);
}

- (id)userInfo {
    env.objc.borrow::<NSProgressHostObject>(this).user_info
}
- (())setUserInfoObject:(id)object forKey:(id)key {
    if key == nil {
        return;
    }
    let info = env.objc.borrow::<NSProgressHostObject>(this).user_info;
    if info == nil {
        let dict: id = msg_class![env; NSMutableDictionary dictionary];
        let dict: id = msg![env; dict copy];
        env.objc
            .borrow_mut::<NSProgressHostObject>(this)
            .user_info = dict;
        retain(env, object);
        () = msg![env; dict setObject:object forKey:key];
        return;
    }
    retain(env, object);
    () = msg![env; info setObject:object forKey:key];
}

// Progress reporting on the current thread is a no-op here: the emulator
// performs all work synchronously and nothing observes the tree.
- (())becomeCurrentWithPendingUnitCount:(i64)_units {}
- (())resignCurrent {}
- (())addChild:(id)_child withPendingUnitCount:(i64)_units {}
- (())publish {}
- (())unpublish {}

- (id)copyWithZone:(crate::objc::NSZonePtr)_zone {
    let source = env.objc.borrow::<NSProgressHostObject>(this);
    let total = source.total_unit_count;
    let completed = source.completed_unit_count;
    let cancelled = source.cancelled;
    let paused = source.paused;
    let pausable = source.pausable;
    let indeterminate = source.indeterminate;
    let kind = source.kind;
    let description = source.localized_description;
    let user_info = source.user_info;
    drop(source);
    let copy: id = msg![env; this alloc];
    let copy: id = msg![env; copy initWithParent:nil userInfo:user_info];
    {
        let host = env.objc.borrow_mut::<NSProgressHostObject>(copy);
        host.total_unit_count = total;
        host.completed_unit_count = completed;
        host.cancelled = cancelled;
        host.paused = paused;
        host.pausable = pausable;
        host.indeterminate = indeterminate;
        host.kind = kind;
        host.localized_description = description;
    }
    retain(env, kind);
    retain(env, description);
    copy
}

- (())dealloc {
    let (kind, localized_description, user_info, cancellation_handler, pausing_handler, resuming_handler) = {
        let host = env.objc.borrow::<NSProgressHostObject>(this);
        (
            host.kind,
            host.localized_description,
            host.user_info,
            host.cancellation_handler,
            host.pausing_handler,
            host.resuming_handler,
        )
    };
    release(env, kind);
    release(env, localized_description);
    release(env, user_info);
    release(env, cancellation_handler);
    release(env, pausing_handler);
    release(env, resuming_handler);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

};
