/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSAttributedString` and `NSMutableAttributedString`.
//!
//! Chrome (and many other apps) use attributed strings for styled text.
//! We store the plain text plus an ordered attribute dictionary per range.
//! Styles are not rendered (our text stack draws plain text), but the
//! object model is complete: ranges, attributes, mutable editing.

use crate::frameworks::foundation::{NSRange, NSInteger, NSUInteger};
use crate::frameworks::foundation::ns_string::NSUTF8StringEncoding;
use crate::mem::MutPtr;
use crate::objc::{id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, NSZonePtr};
use crate::Environment;

/// Host object: text plus `(range, attrs)` pairs, non-overlapping, sorted.
#[derive(Default)]
pub struct NSAttributedStringHostObject {
    text: id, // NSString*
    /// Sorted by range.location.
    runs: Vec<(NSRange, id /* NSDictionary* */)>,
}
impl crate::objc::HostObject for NSAttributedStringHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSAttributedString: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this, Box::<NSAttributedStringHostObject>::default(), &mut env.mem)
}

+ (id)attributedStringWithString:(id)string { // NSString*
    let new: id = msg_class![env; NSAttributedString alloc];
    let new: id = msg![env; new initWithString:string];
    let _: () = msg![env; new autorelease];
    new
}

- (id)init {
    let text: id = msg_class![env; NSString new];
    let this2: id = msg![env; this initWithString:text];
    let _: () = msg![env; text release];
    this2
}

- (id)initWithString:(id)string { // NSString*
    msg![env; this initWithString:string attributes:nil]
}

- (id)initWithString:(id)string attributes:(id)attributes { // (NSString*, NSDictionary*)
    if string == nil {
        let _: () = msg![env; this release];
        return nil;
    }
    let retained_string = retain(env, string);
    let retained_attrs = if attributes != nil {
        let len: NSUInteger = msg![env; string length];
        let attrs = retain(env, attributes);
        Some((NSRange { location: 0, length: len }, attrs))
    } else {
        None
    };
    let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
    host.text = retained_string;
    if let Some((range, attrs)) = retained_attrs {
        host.runs.push((range, attrs));
    }
    this
}

- (id)initWithAttributedString:(id)other {
    if other == nil {
        let _: () = msg![env; this release];
        return nil;
    }
    let text: id = msg![env; other string];
    // Clone runs from other if it's an NSAttributedString
    let other_runs: Vec<(NSRange, id)> = {
        if let Some(other_host) = env.objc.try_borrow::<NSAttributedStringHostObject>(other) {
            other_host.runs.iter().map(|(r, a)| (*r, retain(env, *a))).collect()
        } else {
            Vec::new()
        }
    };
    let this2: id = msg![env; this initWithString:text];
    if this2 != nil && !other_runs.is_empty() {
        let mut host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this2);
        host.runs = other_runs;
    } else {
        for (_, a) in other_runs {
            release(env, a);
        }
    }
    this2
}

- (id)initWithData:(id)data options:(id)_options documentAttributes:(MutPtr<id>)_doc_attrs error:(MutPtr<id>)_error {
    // HTML/RTF import: fall back to interpreting the data as UTF-8 text.
    let string: id = msg_class![env; NSString alloc];
    let string: id = msg![env; string initWithData:data encoding:NSUTF8StringEncoding];
    if string == nil {
        let _: () = msg![env; this release];
        return nil;
    }
    let _: () = msg![env; string autorelease];
    msg![env; this initWithString:string]
}

- (NSUInteger)length {
    let text = env.objc.borrow::<NSAttributedStringHostObject>(this).text;
    msg![env; text length]
}

- (id)string {
    env.objc.borrow::<NSAttributedStringHostObject>(this).text
}

// ---- attribute access ----

- (id)attributesAtIndex:(NSUInteger)location effectiveRange:(MutPtr<NSRange>)range_ptr {
    let host = env.objc.borrow::<NSAttributedStringHostObject>(this);
    for (range, attrs) in host.runs.iter() {
        if location >= range.location && location < range.location + range.length {
            if !range_ptr.is_null() {
                env.mem.write(range_ptr, *range);
            }
            return *attrs;
        }
    }
    if !range_ptr.is_null() {
        env.mem.write(range_ptr, NSRange { location: location, length: 0 });
    }
    nil
}

- (id)attribute:(id)name atIndex:(NSUInteger)location effectiveRange:(MutPtr<NSRange>)range_ptr {
    let attrs: id = msg![env; this attributesAtIndex:location effectiveRange:range_ptr];
    if attrs == nil { return nil; }
    msg![env; attrs objectForKey:name]
}

- (id)attributedSubstringFromRange:(NSRange)range {
    // Snapshot text and runs
    let (text, runs) = {
        let host = env.objc.borrow::<NSAttributedStringHostObject>(this);
        (host.text, host.runs.clone())
    };
    let sub: id = msg![env; text substringWithRange:range];
    // Collect overlapping runs and adjust to subrange coordinates
    let mut new_runs: Vec<(NSRange, id)> = Vec::new();
    for (r, attrs) in runs {
        let overlap_start = r.location.max(range.location);
        let overlap_end = (r.location + r.length).min(range.location + range.length);
        if overlap_start < overlap_end {
            let retained = retain(env, attrs);
            new_runs.push((NSRange { location: overlap_start - range.location, length: overlap_end - overlap_start }, retained));
        }
    }
    let new: id = msg_class![env; NSAttributedString alloc];
    let new: id = msg![env; new initWithString:sub];
    if new != nil {
        if !new_runs.is_empty() {
            env.objc.borrow_mut::<NSAttributedStringHostObject>(new).runs = new_runs;
        }
    } else {
        for (_, a) in new_runs {
            release(env, a);
        }
    }
    new
}

- (bool)isEqualToAttributedString:(id)other {
    if other == nil { return false; }
    let a: id = msg![env; this string];
    let b: id = msg![env; other string];
    let eq: bool = msg![env; a isEqualToString:b];
    eq
}

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let (text, runs) = {
        let host = env.objc.borrow::<NSAttributedStringHostObject>(this);
        (host.text, host.runs.clone())
    };
    let mutable: id = msg_class![env; NSMutableAttributedString alloc];
    let mutable: id = msg![env; mutable initWithString:text];
    if mutable != nil && !runs.is_empty() {
        let mut host_mut = env.objc.borrow_mut::<NSAttributedStringHostObject>(mutable);
        for (range, attrs) in runs {
            let retained = retain(env, attrs);
            host_mut.runs.push((range, retained));
        }
    }
    mutable
}

- (())dealloc {
    let old_text;
    let old_runs;
    {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        old_text = std::mem::replace(&mut host.text, nil);
        old_runs = std::mem::take(&mut host.runs);
    }
    release(env, old_text);
    for (_, attrs) in old_runs {
        release(env, attrs);
    }
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation NSMutableAttributedString: NSAttributedString

// ---- mutable editing ----

- (())setAttributedString:(id)other {
    if other == nil { return; }
    let text: id = msg![env; other string];
    let new_text = retain(env, text);
    let other_runs: Vec<(NSRange, id)> = if let Some(other_host) = env.objc.try_borrow::<NSAttributedStringHostObject>(other) {
        other_host.runs.iter().map(|(r, a)| (*r, retain(env, *a))).collect()
    } else {
        Vec::new()
    };
    let old_text;
    let old_runs;
    {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        old_text = std::mem::replace(&mut host.text, new_text);
        old_runs = std::mem::take(&mut host.runs);
    }
    release(env, old_text);
    for (_, attrs) in old_runs {
        release(env, attrs);
    }
    if !other_runs.is_empty() {
        env.objc.borrow_mut::<NSAttributedStringHostObject>(this).runs = other_runs;
    }
}

- (())addAttribute:(id)name value:(id)value range:(NSRange)range {
    if name == nil || value == nil || range.length == 0 { return; }
    let existing: id = msg![env; this attributesAtIndex:(range.location) effectiveRange:(MutPtr::null())];
    let dict: id = if existing != nil {
        let mutable: id = msg![env; existing mutableCopy];
        let _: () = msg![env; mutable setObject:value forKey:name];
        mutable
    } else {
        let dict: id = msg_class![env; NSMutableDictionary new];
        let _: () = msg![env; dict setObject:value forKey:name];
        dict
    };
    // Remove overlapping runs, preserving non-overlapping fragments
    let old_runs = {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        std::mem::take(&mut host.runs)
    };
    let mut new_runs: Vec<(NSRange, id)> = Vec::new();
    let mut to_release: Vec<id> = Vec::new();
    for (r, attrs) in old_runs {
        if r.location + r.length <= range.location || r.location >= range.location + range.length {
            new_runs.push((r, attrs));
        } else {
            if r.location < range.location {
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: r.location, length: range.location - r.location }, retained));
            }
            if r.location + r.length > range.location + range.length {
                let tail_start = range.location + range.length;
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: tail_start, length: (r.location + r.length) - tail_start }, retained));
            }
            to_release.push(attrs);
        }
    }
    for a in to_release {
        release(env, a);
    }
    new_runs.push((range, dict));
    new_runs.sort_by_key(|(r, _)| r.location);
    env.objc.borrow_mut::<NSAttributedStringHostObject>(this).runs = new_runs;
}

- (())removeAttribute:(id)name range:(NSRange)range {
    if name == nil || range.length == 0 { return; }
    let old_runs = {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        std::mem::take(&mut host.runs)
    };
    let mut new_runs: Vec<(NSRange, id)> = Vec::new();
    for (r, attrs) in old_runs {
        if r.location + r.length <= range.location || r.location >= range.location + range.length {
            new_runs.push((r, attrs));
        } else {
            let contains: bool = {
                let v: id = msg![env; attrs objectForKey:name];
                v != nil
            };
            if !contains {
                new_runs.push((r, attrs));
                continue;
            }
            if r.location < range.location {
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: r.location, length: range.location - r.location }, retained));
            }
            let middle_start = r.location.max(range.location);
            let middle_end = (r.location + r.length).min(range.location + range.length);
            if middle_start < middle_end {
                let mutable: id = msg![env; attrs mutableCopy];
                let _: () = msg![env; mutable removeObjectForKey:name];
                new_runs.push((NSRange { location: middle_start, length: middle_end - middle_start }, mutable));
            }
            if r.location + r.length > range.location + range.length {
                let tail_start = range.location + range.length;
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: tail_start, length: (r.location + r.length) - tail_start }, retained));
            }
            release(env, attrs);
        }
    }
    new_runs.sort_by_key(|(r, _)| r.location);
    env.objc.borrow_mut::<NSAttributedStringHostObject>(this).runs = new_runs;
}

- (())replaceCharactersInRange:(NSRange)range withString:(id)string {
    if string == nil { return; }
    let new_len: NSUInteger = msg![env; string length];
    let delta: NSInteger = new_len as NSInteger - range.length as NSInteger;
    let old_text: id = msg![env; this string];
    let new_text: id = msg![env; old_text stringByReplacingCharactersInRange:range withString:string];
    let stored_text = retain(env, new_text);
    let old_runs = {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        let _old = std::mem::replace(&mut host.text, stored_text);
        // _old is same as old_text's retained storage, we will release old_text below
        std::mem::take(&mut host.runs)
    };
    release(env, old_text);
    let mut new_runs: Vec<(NSRange, id)> = Vec::new();
    for (r, attrs) in old_runs {
        if r.location + r.length <= range.location {
            new_runs.push((r, attrs));
        } else if r.location >= range.location + range.length {
            let mut nr = r;
            nr.location = (nr.location as NSInteger + delta) as NSUInteger;
            new_runs.push((nr, attrs));
        } else {
            if r.location < range.location {
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: r.location, length: range.location - r.location }, retained));
            }
            if r.location + r.length > range.location + range.length {
                let tail_start = range.location + range.length;
                let tail_len = (r.location + r.length) - tail_start;
                let new_loc = (tail_start as NSInteger + delta) as NSUInteger;
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: new_loc, length: tail_len }, retained));
            }
            release(env, attrs);
        }
    }
    new_runs.retain(|(r, _)| r.length > 0);
    new_runs.sort_by_key(|(r, _)| r.location);
    env.objc.borrow_mut::<NSAttributedStringHostObject>(this).runs = new_runs;
}

- (())setAttributes:(id)attributes range:(NSRange)range {
    if range.length == 0 { return; }
    let new_attrs = if attributes != nil { retain(env, attributes) } else { nil };
    let old_runs = {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        std::mem::take(&mut host.runs)
    };
    let mut new_runs: Vec<(NSRange, id)> = Vec::new();
    let mut to_release: Vec<id> = Vec::new();
    for (r, attrs) in old_runs {
        if r.location + r.length <= range.location || r.location >= range.location + range.length {
            new_runs.push((r, attrs));
        } else {
            if r.location < range.location {
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: r.location, length: range.location - r.location }, retained));
            }
            if r.location + r.length > range.location + range.length {
                let tail_start = range.location + range.length;
                let retained = retain(env, attrs);
                new_runs.push((NSRange { location: tail_start, length: (r.location + r.length) - tail_start }, retained));
            }
            to_release.push(attrs);
        }
    }
    for a in to_release {
        release(env, a);
    }
    if new_attrs != nil {
        new_runs.push((range, new_attrs));
    }
    new_runs.sort_by_key(|(r, _)| r.location);
    env.objc.borrow_mut::<NSAttributedStringHostObject>(this).runs = new_runs;
}

@end

};
