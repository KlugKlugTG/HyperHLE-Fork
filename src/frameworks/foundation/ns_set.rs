/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The `NSSet` class cluster, including `NSMutableSet` and `NSCountedSet`.

use super::ns_array;
use super::ns_dictionary::DictionaryHostObject;
use super::ns_enumerator::{fast_enumeration_helper, NSFastEnumerationState};
use super::NSUInteger;
use crate::abi::DotDotDot;
use crate::environment::Environment;
use crate::mem::{ConstPtr, MutPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};

/// Belongs to _touchHLE_NSSet
#[derive(Debug, Default)]
struct SetHostObject {
    dict: DictionaryHostObject,
}
impl HostObject for SetHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// NSSet is an abstract class. A subclass must provide:
// - (NSUInteger)count;
// - (id)member:(id)object;
// - (NSEnumerator*)objectEnumerator;
// We can pick whichever subclass we want for the various alloc methods.
// For the time being, that will always be _touchHLE_NSSet.
@implementation NSSet: NSObject

+ (id)allocWithZone:(NSZonePtr)zone {
    // NSSet might be subclassed by something which needs allocWithZone:
    // to have the normal behaviour. Unimplemented: call superclass alloc then.
    if this != env.objc.get_known_class("NSSet", &mut env.mem) {
        log!(
            "Warning: [+ {:?} allocWithZone:{:?}] called on NSSet subclass; falling back to _touchHLE_NSSet.",
            this,
            zone
        );
    }
    msg_class![env; _touchHLE_NSSet allocWithZone:zone]
}

+ (id)set {
    let set: id = msg![env; this new];
    autorelease(env, set)
}

+ (id)setWithArray:(id)array {
    let count: NSUInteger = msg![env; array count];
    let new: id = msg![env; this alloc];
    let mut dict = <DictionaryHostObject as Default>::default();

    for i in 0..count {
        let object: id = msg![env; array objectAtIndex:i];
        let null: id = msg_class![env; NSNull null];
        dict.insert(env, object, null, /* copy_key: */ false);
    }
    env.objc.borrow_mut::<SetHostObject>(new).dict = dict;
    autorelease(env, new)
}

+ (id)setWithSet:(id)object {
    // assert!(object != nil);
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithObject:object];
    autorelease(env, new)
}

+ (id)setWithObject:(id)object {
    if object == nil {
        log!("Warning: +[NSSet setWithObject:nil]; returning empty set.");
        let new: id = msg![env; this alloc];
        let new: id = msg![env; new init];
        return autorelease(env, new);
    }
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithObject:object];
    autorelease(env, new)
}

+ (id)setWithObjects:(id)first_obj, ...args {
    if this != env.objc.get_known_class("NSSet", &mut env.mem) {
        log!(
            "Warning: +[{:?} setWithObjects:...] called on NSSet subclass; falling back to _touchHLE_NSSet.",
            this
        );
    }
    let new: id = msg_class![env; _touchHLE_NSSet alloc];
    env.objc.borrow_mut::<SetHostObject>(new).dict = set_from_objects(env, first_obj, args);
    autorelease(env, new)
}

// Apple: "Creates and returns a set containing a specified number of objects
// from a given C array of objects."
// https://developer.apple.com/documentation/foundation/nsset/1574811-setwithobjects
+ (id)setWithObjects:(ConstPtr<id>)objects count:(NSUInteger)count {
    let new: id = msg_class![env; _touchHLE_NSSet alloc];
    let new: id = msg![env; new initWithObjects:objects count:count];
    autorelease(env, new)
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (bool)containsObject:(id)object {
    let enumerator: id = msg![env; this objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            return false;
        }
        if msg![env; next isEqual:object] {
            return true;
        }
    }
}

- (i32)intValue {
        let count: NSUInteger = msg![env; this count];
        count as i32
}

@end

// NSMutableSet is an abstract class. A subclass must provide everything
// NSSet provides, plus:
// - (void)addObject:(id)object;
// - (void)removeObject:(id)object;
// Note that it inherits from NSSet, so we must ensure we override any default
// methods that would be inappropriate for mutability.
@implementation NSMutableSet: NSSet

+ (id)allocWithZone:(NSZonePtr)zone {
    // NSSet might be subclassed by something which needs allocWithZone:
    // to have the normal behaviour. Unimplemented: call superclass alloc then.
    if this != env.objc.get_known_class("NSMutableSet", &mut env.mem) {
        log!(
            "Warning: [+ {:?} allocWithZone:{:?}] called on NSMutableSet subclass; falling back to _touchHLE_NSMutableSet.",
            this,
            zone
        );
    }
    msg_class![env; _touchHLE_NSMutableSet allocWithZone:zone]
}

+ (id)setWithCapacity:(NSUInteger)numItems {
    if this != env.objc.get_known_class("NSMutableSet", &mut env.mem) {
        log!(
            "Warning: +[{:?} setWithCapacity:{}] called on NSMutableSet subclass; falling back to _touchHLE_NSMutableSet.",
            this,
            numItems
        );
    }
    let new: id = msg_class![env; _touchHLE_NSMutableSet alloc];
    let new: id = msg![env; new initWithCapacity:numItems];
    autorelease(env, new)
}

+ (id)setWithObjects:(id)first_obj, ...args {
    if this != env.objc.get_known_class("NSMutableSet", &mut env.mem) {
        log!(
            "Warning: +[{:?} setWithObjects:...] called on NSMutableSet subclass; falling back to _touchHLE_NSMutableSet.",
            this
        );
    }
    let new: id = msg_class![env; _touchHLE_NSMutableSet alloc];
    env.objc.borrow_mut::<SetHostObject>(new).dict = set_from_objects(env, first_obj, args);
    autorelease(env, new)
}

+ (id)setWithObjects:(ConstPtr<id>)objects count:(NSUInteger)count {
    let new: id = msg_class![env; _touchHLE_NSMutableSet alloc];
    let new: id = msg![env; new initWithObjects:objects count:count];
    autorelease(env, new)
}

// NSCopying implementation
// NSMutableSet's -copyWithZone: must produce an immutable NSSet that
// snapshots the receiver. We materialise the elements via -allObjects
// (which is implemented for our private subclass) and reuse the
// +[NSSet setWithArray:] code path to build a fresh immutable set.
- (id)copyWithZone:(NSZonePtr)_zone {
    let objects: id = msg![env; this allObjects];
    msg_class![env; NSSet setWithArray:objects]
}

@end

// Our private subclass that is the single implementation of NSSet for the
// time being.
@implementation _touchHLE_NSSet: NSSet

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SetHostObject {
        dict: Default::default(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithObject:(id)object {
    let null: id = msg_class![env; NSNull null];

    let mut dict = <DictionaryHostObject as Default>::default();
    dict.insert(env, object, null, /* copy_key: */ false);

    env.objc.borrow_mut::<SetHostObject>(this).dict = dict;

    this
}

- (id)initWithObjects:(id)first_obj, ...args {
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_objects(env, first_obj, args);
    this
}

- (id)initWithObjects:(ConstPtr<id>)objects count:(NSUInteger)count {
    env.objc.borrow_mut::<SetHostObject>(this).dict =
        set_from_c_array(env, objects, count);
    this
}

- (id)initWithArray:(id)array { // NSArray*
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_array(env, array);
    this
}

- (id)initWithSet:(id)other { // NSSet*
    // Apple docs (NSSet "Creating a Set"): "Returns a newly initialized set
    // using the objects from another given set." The objects are NOT copied
    // — they're retained, exactly like -initWithArray:.
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_set(env, other, false);
    this
}

- (id)initWithSet:(id)other copyItems:(bool)copy_items { // NSSet*
    // Apple docs: when `flag` is YES, each object is sent -copyWithZone:nil
    // and the copy is added to the new set (objects must conform to
    // NSCopying — non-conforming objects raise NSInvalidArgumentException).
    env.objc.borrow_mut::<SetHostObject>(this).dict =
        set_from_set(env, other, copy_items);
    this
}

- (())dealloc {
    std::mem::take(&mut env.objc.borrow_mut::<SetHostObject>(this).dict).release(env);
    env.objc.dealloc_object(this, &mut env.mem)
}

// TODO: more init methods, etc

// TODO: accessors
- (NSUInteger)count {
    env.objc.borrow_mut::<SetHostObject>(this).dict.count
}

- (id)anyObject {
    let object_or_none = env.objc.borrow_mut::<SetHostObject>(this).dict.iter_keys().next();
    match object_or_none {
        Some(object) => object,
        None => nil
    }
}

- (id)allObjects {
    let objects = env.objc.borrow_mut::<SetHostObject>(this).dict.iter_keys().collect();
    ns_array::from_vec(env, objects)
}

// Apple: "Returns the object in the set that is equal to a given object, or
// nil if no such object exists."
// https://developer.apple.com/documentation/foundation/nsset/1414703-member
- (id)member:(id)object {
    set_member(env, this, object)
}

- (id)objectEnumerator { // NSEnumerator*
    let array: id = msg![env; this allObjects];
    msg![env; array objectEnumerator]
}

// NSFastEnumeration implementation
- (NSUInteger)countByEnumeratingWithState:(MutPtr<NSFastEnumerationState>)state
                                  objects:(MutPtr<id>)stackbuf
                                    count:(NSUInteger)len {
    // We assume that order in which objects are reported is consistent
    // between calls!
    let objects: id = msg![env; this allObjects];
    let count: NSUInteger = msg![env; objects count];
    fast_enumeration_helper(env, this, |env, idx| {
        if idx < count {
            msg![env; objects objectAtIndex:idx]
        } else {
            nil
        }
    }, state, stackbuf, len)
}

@end

// Our private subclass that is the single implementation of NSMutableSet for
// the time being.
@implementation _touchHLE_NSMutableSet: NSMutableSet

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SetHostObject {
        dict: Default::default(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

// NSCopying implementation
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (id)initWithCapacity:(NSUInteger)_numItems {
    // We ignore the requested capacity as Rust's internal data structures
    // handle resizing automatically.
    env.objc.borrow_mut::<SetHostObject>(this).dict = Default::default();
    this
}

- (id)initWithObject:(id)object {
    let null: id = msg_class![env; NSNull null];

    let mut dict = <DictionaryHostObject as Default>::default();
    dict.insert(env, object, null, /* copy_key: */ false);

    env.objc.borrow_mut::<SetHostObject>(this).dict = dict;

    this
}

- (id)initWithObjects:(id)first_obj, ...args {
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_objects(env, first_obj, args);
    this
}

- (id)initWithObjects:(ConstPtr<id>)objects count:(NSUInteger)count {
    env.objc.borrow_mut::<SetHostObject>(this).dict =
        set_from_c_array(env, objects, count);
    this
}

- (id)initWithArray:(id)array { // NSArray*
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_array(env, array);
    this
}

- (id)initWithSet:(id)other { // NSSet*
    // Apple docs (NSSet "Creating a Set"): "Returns a newly initialized set
    // using the objects from another given set." The objects are NOT copied
    // — they're retained, exactly like -initWithArray:.
    env.objc.borrow_mut::<SetHostObject>(this).dict = set_from_set(env, other, false);
    this
}

- (id)initWithSet:(id)other copyItems:(bool)copy_items { // NSSet*
    // Apple docs: when `flag` is YES, each object is sent -copyWithZone:nil
    // and the copy is added to the new set (objects must conform to
    // NSCopying — non-conforming objects raise NSInvalidArgumentException).
    env.objc.borrow_mut::<SetHostObject>(this).dict =
        set_from_set(env, other, copy_items);
    this
}

- (())dealloc {
    std::mem::take(&mut env.objc.borrow_mut::<SetHostObject>(this).dict).release(env);
    env.objc.dealloc_object(this, &mut env.mem)
}

// TODO: init methods etc

- (NSUInteger)count {
    env.objc.borrow_mut::<SetHostObject>(this).dict.count
}

- (())bodyType {

}

- (())b2body {

}

- (id)anyObject {
    let object_or_none = env.objc.borrow_mut::<SetHostObject>(this).dict.iter_keys().next();
    match object_or_none {
        Some(object) => object,
        None => nil
    }
}

- (id)allObjects {
    let objects = env.objc.borrow_mut::<SetHostObject>(this).dict.iter_keys().collect();
    ns_array::from_vec(env, objects)
}

- (id)member:(id)object {
    set_member(env, this, object)
}

- (id)objectEnumerator { // NSEnumerator*
    let array: id = msg![env; this allObjects];
    msg![env; array objectEnumerator]
}

// NSFastEnumeration implementation
- (NSUInteger)countByEnumeratingWithState:(MutPtr<NSFastEnumerationState>)state
                                  objects:(MutPtr<id>)stackbuf
                                    count:(NSUInteger)len {
    // TODO: check that set wasn't mutated!
    // We assume that order in which objects are reported is consistent
    // between calls!
    let objects: id = msg![env; this allObjects];
    let count: NSUInteger = msg![env; objects count];
    fast_enumeration_helper(env, this, |env, idx| {
        if idx < count {
            msg![env; objects objectAtIndex:idx]
        } else {
            nil
        }
    }, state, stackbuf, len)
}

// TODO: more mutation methods

- (())addObject:(id)object {
    let null: id = msg_class![env; NSNull null];
    let mut host_obj: SetHostObject = std::mem::take(env.objc.borrow_mut(this));
    host_obj.dict.insert(env, object, null, /* copy_key: */ false);
    *env.objc.borrow_mut(this) = host_obj;
}

- (())removeObject:(id)object {
    let mut host_obj: SetHostObject = std::mem::take(env.objc.borrow_mut(this));
    host_obj.dict.remove(env, object);
    *env.objc.borrow_mut(this) = host_obj;
}

- (())removeAllObjects {
    let mut old_host_obj = std::mem::replace(
        env.objc.borrow_mut(this),
        SetHostObject {
            dict: Default::default(),
        },
    );
    old_host_obj.dict.release(env);
}

- (())unionSet:(id)other { // NSSet *
    let enumerator: id = msg![env; other objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        () = msg![env; this addObject:next];
    }
}


- (())minusSet:(id)other { // NSSet *
    if other == nil {
        return;
    }
    let enumerator: id = msg![env; other objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        () = msg![env; this removeObject:next];
    }
}

- (())setSet:(id)other { // NSSet *
    () = msg![env; this removeAllObjects];
    () = msg![env; this unionSet:other];
}

- (())intersectSet:(id)other { // NSSet *
    if other == nil {
        () = msg![env; this removeAllObjects];
        return;
    }
    let objects: id = msg![env; this allObjects];
    let count: NSUInteger = msg![env; objects count];
    let mut i: NSUInteger = 0;
    while i < count {
        let object: id = msg![env; objects objectAtIndex:i];
        let contains: bool = msg![env; other containsObject:object];
        if !contains {
            () = msg![env; this removeObject:object];
        }
        i += 1;
    }
}

- (())encodeWithCoder:(id)coder {
    let objects: id = msg![env; this allObjects];
    () = msg![env; objects encodeWithCoder:coder];
}

// Apple: "Adds to the receiving set each object contained in a given array
// that is not already a member." (NSMutableSet addObjectsFromArray:).
// https://developer.apple.com/documentation/foundation/nsmutableset/1408015-addobjectsfromarray
//
// We tolerate `nil` (real Foundation crashes, but every other touchHLE
// container path tolerates nil to keep flaky games alive), and use indexed
// access rather than fast enumeration because some guest array
// implementations don't implement -objectEnumerator yet.
- (())addObjectsFromArray:(id)array { // NSArray *
    if array == nil {
        return;
    }
    let count: NSUInteger = msg![env; array count];
    let mut i: NSUInteger = 0;
    while i < count {
        let object: id = msg![env; array objectAtIndex:i];
        () = msg![env; this addObject:object];
        i += 1;
    }
}

@end

};

/// Helper method shared between `initWithObjects:` of `_touchHLE_NSSet` and
/// `_touchHLE_NSMutableSet`
fn set_from_objects(env: &mut Environment, first_obj: id, args: DotDotDot) -> DictionaryHostObject {
    let null: id = msg_class![env; NSNull null];

    let mut dict = <DictionaryHostObject as Default>::default();
    dict.insert(env, first_obj, null, /* copy_key: */ false);
    let mut varargs = args.start();
    loop {
        let next_arg: id = varargs.next(env);
        if next_arg == nil {
            break;
        }
        dict.insert(env, next_arg, null, /* copy_key: */ false);
    }
    dict
}

/// Helper method shared between `initWithArray:` of `_touchHLE_NSSet` and
/// `_touchHLE_NSMutableSet`. Iterates the given array (which may be `nil`)
/// and inserts each object into a fresh dictionary, mirroring the semantics
/// of `-[NSSet initWithArray:]` documented by Apple.
fn set_from_array(env: &mut Environment, array: id) -> DictionaryHostObject {
    let mut dict = <DictionaryHostObject as Default>::default();
    if array == nil {
        return dict;
    }
    let null: id = msg_class![env; NSNull null];
    let count: NSUInteger = msg![env; array count];
    for i in 0..count {
        let object: id = msg![env; array objectAtIndex:i];
        dict.insert(env, object, null, /* copy_key: */ false);
    }
    dict
}

/// Build a [DictionaryHostObject] populated with the contents of `other`,
/// reached through the public NSSet API so subclasses work transparently.
/// When `copy_items` is true, each object is sent `-copyWithZone:nil` and the
/// returned copy is inserted instead of the original, matching the documented
/// behavior of `-[NSSet initWithSet:copyItems:]`.
fn set_from_set(env: &mut Environment, other: id, copy_items: bool) -> DictionaryHostObject {
    let mut dict = <DictionaryHostObject as Default>::default();
    if other == nil {
        return dict;
    }
    let null: id = msg_class![env; NSNull null];
    let enumerator: id = msg![env; other objectEnumerator];
    if enumerator == nil {
        return dict;
    }
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        if copy_items {
            // -[NSObject copy] returns an owned (+1 retain) copy. Insert it,
            // then release our extra reference so the dictionary's retain is
            // the only one we own.
            let copy: id = msg![env; next copy];
            dict.insert(env, copy, null, /* copy_key: */ false);
            release(env, copy);
        } else {
            dict.insert(env, next, null, /* copy_key: */ false);
        }
    }
    dict
}

/// Build a [DictionaryHostObject] from a C array of `count` object pointers,
/// shared between `-initWithObjects:count:` and `+setWithObjects:count:`.
/// Mirrors Apple's documented `+[NSSet setWithObjects:count:]`.
fn set_from_c_array(
    env: &mut Environment,
    objects: ConstPtr<id>,
    count: NSUInteger,
) -> DictionaryHostObject {
    let null: id = msg_class![env; NSNull null];
    let mut dict = <DictionaryHostObject as Default>::default();
    for i in 0..count {
        let object: id = env.mem.read(objects + i);
        dict.insert(env, object, null, /* copy_key: */ false);
    }
    dict
}

/// Shared implementation of `-[NSSet member:]` for both private subclasses.
/// Returns the stored object equal (by pointer identity or `-isEqual:`) to
/// `object`, or `nil` if there is none. Apple documents `member:` as the
/// canonical equality lookup for set membership.
fn set_member(env: &mut Environment, this: id, object: id) -> id {
    if object == nil {
        return nil;
    }
    let keys: Vec<id> = env
        .objc
        .borrow_mut::<SetHostObject>(this)
        .dict
        .iter_keys()
        .collect();
    for key in keys {
        if key == object {
            return key;
        }
        let eq: bool = msg![env; key isEqual:object];
        if eq {
            return key;
        }
    }
    nil
}
