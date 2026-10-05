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

use crate::abi::{CallFromHost, GuestFunction};
use crate::frameworks::foundation::{NSRange, NSInteger, NSUInteger};
use crate::mem::{ConstVoidPtr, MutPtr};
use crate::frameworks::foundation::ns_string::NSUTF8StringEncoding;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, NSZonePtr,
};
use crate::Environment;

/// Host object: text plus `(range, attrs)` pairs, non-overlapping, sorted.
#[derive(Default)]
pub struct NSAttributedStringHostObject {
    text: id, // NSString*
    /// Sorted by range.location.
    runs: Vec<(NSRange, id /* NSDictionary* */)>,
}
impl crate::objc::HostObject for NSAttributedStringHostObject {}

fn mutable_attribute_copy(env: &mut Environment, attributes: id) -> id {
    let copy: id = msg_class![env; NSMutableDictionary alloc];
    let copy: id = msg![env; copy init];
    if attributes != nil {
        let keys: id = msg![env; attributes allKeys];
        let count: NSUInteger = msg![env; keys count];
        for index in 0..count {
            let key: id = msg![env; keys objectAtIndex:index];
            let value: id = msg![env; attributes objectForKey:key];
            if key != nil && value != nil {
                let _: () = msg![env; copy setObject:value forKey:key];
            }
        }
    }
    retain(env, copy)
}

fn retained_text_and_runs(
    env: &mut Environment,
    source: id,
) -> (id, Vec<(NSRange, id)>) {
    if source == nil {
        return (nil, Vec::new());
    }
    let (text, runs) = {
        let host = env.objc.borrow::<NSAttributedStringHostObject>(source);
        (host.text, host.runs.clone())
    };
    let text = retain(env, text);
    let runs = runs
        .into_iter()
        .map(|(range, attributes)| (range, retain(env, attributes)))
        .collect();
    (text, runs)
}

fn objects_equal(env: &mut Environment, left: id, right: id) -> bool {
    if left == right {
        true
    } else if left == nil || right == nil {
        false
    } else {
        msg![env; left isEqual:right]
    }
}

fn dictionaries_equal(env: &mut Environment, left: id, right: id) -> bool {
    if left == right {
        true
    } else if left == nil || right == nil {
        false
    } else {
        msg![env; left isEqualToDictionary:right]
    }
}

fn attribute_segments(
    env: &mut Environment,
    this: id,
    range: NSRange,
) -> (Vec<(NSRange, id)>, Vec<id>) {
    let (text, runs) = {
        let host = env.objc.borrow::<NSAttributedStringHostObject>(this);
        (host.text, host.runs.clone())
    };
    if text == nil || range.length == 0 {
        return (Vec::new(), Vec::new());
    }
    let string_length: NSUInteger = msg![env; text length];
    let start = range.location.min(string_length);
    let end = range.location.saturating_add(range.length).min(string_length);
    if start >= end {
        return (Vec::new(), Vec::new());
    }
    let empty: id = msg_class![env; NSDictionary dictionary];
    retain(env, empty);
    let mut retained = vec![empty];
    let mut boundaries = vec![start, end];
    for (run, attributes) in &runs {
        if *attributes != nil {
            retain(env, *attributes);
            retained.push(*attributes);
        }
        let run_end = run.location.saturating_add(run.length);
        if run.location > start && run.location < end {
            boundaries.push(run.location);
        }
        if run_end > start && run_end < end {
            boundaries.push(run_end);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut segments: Vec<(NSRange, id)> = Vec::new();
    for pair in boundaries.windows(2) {
        let location = pair[0];
        let segment_end = pair[1];
        if location >= segment_end {
            continue;
        }
        let attributes = runs
            .iter()
            .rfind(|(run, _)| {
                run.location <= location
                    && location < run.location.saturating_add(run.length)
            })
            .map(|(_, attributes)| *attributes)
            .filter(|attributes| *attributes != nil)
            .unwrap_or(empty);
        let segment_range = NSRange {
            location,
            length: segment_end - location,
        };
        if let Some((previous_range, previous_attributes)) = segments.last_mut() {
            if dictionaries_equal(env, *previous_attributes, attributes)
                && previous_range.location.saturating_add(previous_range.length) == location
            {
                previous_range.length += segment_range.length;
                continue;
            }
        }
        segments.push((segment_range, attributes));
    }
    (segments, retained)
}

fn transform_attributes_in_range<F>(
    env: &mut Environment,
    this: id,
    range: NSRange,
    mut transform: F,
) where
    F: FnMut(&mut Environment, id) -> id,
{
    let (segments, retained) = attribute_segments(env, this, range);
    let updated: Vec<(NSRange, id)> = segments
        .into_iter()
        .map(|(segment_range, attributes)| (segment_range, transform(env, attributes)))
        .collect();
    {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        host.runs.extend(updated);
        host.runs.sort_by_key(|(segment_range, _)| segment_range.location);
    }
    for object in retained {
        release(env, object);
    }
}

fn segment_value(env: &mut Environment, attributes: id, key: Option<id>) -> id {
    if let Some(key) = key {
        if attributes == nil {
            nil
        } else {
            msg![env; attributes objectForKey:key]
        }
    } else {
        attributes
    }
}

fn segment_values_equal(env: &mut Environment, left: id, right: id, key: Option<id>) -> bool {
    if key.is_some() {
        objects_equal(env, left, right)
    } else {
        dictionaries_equal(env, left, right)
    }
}

fn adjacent_segments_equal(
    env: &mut Environment,
    left_range: NSRange,
    left_attributes: id,
    right_range: NSRange,
    right_attributes: id,
    key: Option<id>,
) -> bool {
    if left_range.location.saturating_add(left_range.length) != right_range.location {
        return false;
    }
    let left_value = segment_value(env, left_attributes, key);
    let right_value = segment_value(env, right_attributes, key);
    segment_values_equal(env, left_value, right_value, key)
}

fn effective_attribute_segment(
    env: &mut Environment,
    this: id,
    location: NSUInteger,
    range_limit: NSRange,
    key: Option<id>,
) -> (id, NSRange) {
    let (segments, retained) = attribute_segments(env, this, range_limit);
    let found = segments.iter().position(|(range, _)| {
        range.location <= location && location < range.location.saturating_add(range.length)
    });
    let result = if let Some(index) = found {
        let value = segment_value(env, segments[index].1, key);
        let mut first = index;
        let mut last = index;
        while first > 0 {
            let (previous_range, previous_attributes) = segments[first - 1];
            let (current_range, current_attributes) = segments[first];
            if !adjacent_segments_equal(
                env,
                previous_range,
                previous_attributes,
                current_range,
                current_attributes,
                key,
            ) {
                break;
            }
            first -= 1;
        }
        while last + 1 < segments.len() {
            let (current_range, current_attributes) = segments[last];
            let (next_range, next_attributes) = segments[last + 1];
            if !adjacent_segments_equal(
                env,
                current_range,
                current_attributes,
                next_range,
                next_attributes,
                key,
            ) {
                break;
            }
            last += 1;
        }
        let start = segments[first].0.location;
        let end = segments[last]
            .0
            .location
            .saturating_add(segments[last].0.length);
        (
            value,
            NSRange {
                location: start,
                length: end.saturating_sub(start),
            },
        )
    } else {
        (
            nil,
            NSRange {
                location,
                length: 0,
            },
        )
    };
    for object in retained {
        release(env, object);
    }
    result
}

pub(crate) fn attributes_at_index_with_range(
    env: &mut Environment,
    this: id,
    location: NSUInteger,
    range_limit: NSRange,
) -> (id, NSRange) {
    effective_attribute_segment(env, this, location, range_limit, None)
}

pub(crate) fn attribute_at_index_with_range(
    env: &mut Environment,
    this: id,
    key: id,
    location: NSUInteger,
    range_limit: NSRange,
) -> (id, NSRange) {
    effective_attribute_segment(env, this, location, range_limit, Some(key))
}

fn invoke_attribute_block(
    env: &mut Environment,
    block: id,
    value: id,
    range: NSRange,
    stop: MutPtr<u8>,
) {
    if block == nil {
        return;
    }
    let invoke_ptr: u32 = env.mem.read(block.cast::<u32>() + 3u32);
    if invoke_ptr == 0 {
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_ptr);
    let block_arg: ConstVoidPtr = block.cast::<std::ffi::c_void>().cast_const();
    let _: () = invoke.call_from_host(env, (block_arg, value, range, stop));
}

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
    msg![env; this initWithString:text]
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
        let attrs: id = msg![env; attributes copy];
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
    let (text, runs) = retained_text_and_runs(env, other);
    if text == nil {
        let _: () = msg![env; this release];
        return nil;
    }
    let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
    host.text = text;
    host.runs = runs;
    this
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
    let text: id = msg![env; this string];
    let length: NSUInteger = msg![env; text length];
    if location >= length {
        if !range_ptr.is_null() {
            env.mem.write(range_ptr, NSRange { location, length: 0 });
        }
        return nil;
    }
    let (attributes, range) = attributes_at_index_with_range(
        env,
        this,
        location,
        NSRange {
            location: 0,
            length,
        },
    );
    if !range_ptr.is_null() {
        env.mem.write(range_ptr, range);
    }
    attributes
}

- (id)attribute:(id)name atIndex:(NSUInteger)location effectiveRange:(MutPtr<NSRange>)range_ptr {
    let text: id = msg![env; this string];
    let length: NSUInteger = msg![env; text length];
    if location >= length {
        if !range_ptr.is_null() {
            env.mem.write(range_ptr, NSRange { location, length: 0 });
        }
        return nil;
    }
    let (value, range) = attribute_at_index_with_range(
        env,
        this,
        name,
        location,
        NSRange {
            location: 0,
            length,
        },
    );
    if !range_ptr.is_null() {
        env.mem.write(range_ptr, range);
    }
    value
}

- (())enumerateAttributesInRange:(NSRange)enumeration_range
                         options:(NSUInteger)options
                      usingBlock:(id)block {
    if block == nil {
        return;
    }
    let (mut segments, retained) = attribute_segments(env, this, enumeration_range);
    if options & (1 << 1) != 0 {
        segments.reverse();
    }
    let stop_allocation = env.mem.alloc(4);
    let stop: MutPtr<u8> = stop_allocation.cast();
    for (range, attributes) in segments {
        env.mem.write(stop, 0u8);
        invoke_attribute_block(env, block, attributes, range, stop);
        if env.mem.read(stop) != 0 {
            break;
        }
    }
    env.mem.free(stop_allocation);
    for object in retained {
        release(env, object);
    }
}

- (())enumerateAttribute:(id)name
                   inRange:(NSRange)enumeration_range
                    options:(NSUInteger)options
                 usingBlock:(id)block {
    if block == nil {
        return;
    }
    let (segments, retained) = attribute_segments(env, this, enumeration_range);
    let mut values: Vec<(NSRange, id)> = Vec::new();
    for (range, attributes) in segments {
        let value: id = msg![env; attributes objectForKey:name];
        if let Some((previous_range, previous_value)) = values.last_mut() {
            let adjacent =
                previous_range.location.saturating_add(previous_range.length) == range.location;
            if adjacent && objects_equal(env, *previous_value, value) {
                previous_range.length += range.length;
                continue;
            }
        }
        values.push((range, value));
    }
    if options & (1 << 1) != 0 {
        values.reverse();
    }
    let stop_allocation = env.mem.alloc(4);
    let stop: MutPtr<u8> = stop_allocation.cast();
    for (range, value) in values {
        env.mem.write(stop, 0u8);
        invoke_attribute_block(env, block, value, range, stop);
        if env.mem.read(stop) != 0 {
            break;
        }
    }
    env.mem.free(stop_allocation);
    for object in retained {
        release(env, object);
    }
}

- (id)attributedSubstringFromRange:(NSRange)range {
    let text = env.objc.borrow::<NSAttributedStringHostObject>(this).text;
    let text_length: NSUInteger = msg![env; text length];
    let Some(end) = range.location.checked_add(range.length) else {
        return nil;
    };
    if end > text_length {
        return nil;
    }
    let substring: id = msg![env; text substringWithRange:range];
    let new: id = msg_class![env; NSAttributedString alloc];
    let initialized: id = msg![env; new initWithString:substring];
    if initialized == nil {
        return nil;
    }
    let (segments, retained) = attribute_segments(env, this, range);
    let new_runs = segments
        .into_iter()
        .map(|(mut segment_range, attributes)| {
            segment_range.location -= range.location;
            (segment_range, retain(env, attributes))
        })
        .collect();
    env.objc
        .borrow_mut::<NSAttributedStringHostObject>(new)
        .runs = new_runs;
    for object in retained {
        release(env, object);
    }
    autorelease(env, new)
}

- (bool)isEqualToAttributedString:(id)other {
    if other == nil { return false; }
    let a: id = msg![env; this string];
    let b: id = msg![env; other string];
    let eq: bool = msg![env; a isEqualToString:b];
    eq
}

- (id)copyWithZone:(NSZonePtr)_zone {
    let copy: id = msg_class![env; NSAttributedString alloc];
    msg![env; copy initWithAttributedString:this]
}
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let copy: id = msg_class![env; NSMutableAttributedString alloc];
    msg![env; copy initWithAttributedString:this]
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
    if other == nil {
        return;
    }
    let (new_text, new_runs) = retained_text_and_runs(env, other);
    let old_text;
    let old_runs;
    {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        old_text = std::mem::replace(&mut host.text, new_text);
        old_runs = std::mem::replace(&mut host.runs, new_runs);
    }
    release(env, old_text);
    for (_, attrs) in old_runs {
        release(env, attrs);
    }
}

- (())addAttribute:(id)name value:(id)value range:(NSRange)range {
    if name == nil || value == nil || range.length == 0 {
        return;
    }
    transform_attributes_in_range(env, this, range, |env, attributes| {
        let copy = mutable_attribute_copy(env, attributes);
        let _: () = msg![env; copy setObject:value forKey:name];
        copy
    });
}

- (())addAttributes:(id)attributes range:(NSRange)range {
    if attributes == nil || range.length == 0 {
        return;
    }
    let keys: id = msg![env; attributes allKeys];
    let key_count: NSUInteger = msg![env; keys count];
    if key_count == 0 {
        return;
    }
    transform_attributes_in_range(env, this, range, |env, existing| {
        let copy = mutable_attribute_copy(env, existing);
        for index in 0..key_count {
            let key: id = msg![env; keys objectAtIndex:index];
            let value: id = msg![env; attributes objectForKey:key];
            if key != nil && value != nil {
                let _: () = msg![env; copy setObject:value forKey:key];
            }
        }
        copy
    });
}

- (())removeAttribute:(id)name range:(NSRange)range {
    if name == nil || range.length == 0 {
        return;
    }
    transform_attributes_in_range(env, this, range, |env, attributes| {
        let copy = mutable_attribute_copy(env, attributes);
        let _: () = msg![env; copy removeObjectForKey:name];
        copy
    });
}

- (())replaceCharactersInRange:(NSRange)range withString:(id)string {
    let new_len: NSUInteger = msg![env; string length];
    let delta: NSInteger = new_len as NSInteger - range.length as NSInteger;
    let old_text: id = msg![env; this string];
    let new_text: id = msg![env; old_text stringByReplacingCharactersInRange:range withString:string];
    let stored_text = retain(env, new_text);
    let _ = new_text;
    {
        let host = env.objc.borrow_mut::<NSAttributedStringHostObject>(this);
        host.text = stored_text;
        for (r, _attrs) in host.runs.iter_mut() {
            if r.location >= range.location + range.length {
                r.location = (r.location as NSInteger + delta) as NSUInteger;
            } else if r.location + r.length > range.location {
                r.length = range.location.saturating_sub(r.location);
            }
        }
        host.runs.retain(|(r, _)| r.length > 0 || new_len == 0);
    }
    release(env, old_text);
}

- (())setAttributes:(id)attributes range:(NSRange)range {
    if range.length == 0 {
        return;
    }
    transform_attributes_in_range(env, this, range, |env, _existing| {
        mutable_attribute_copy(env, attributes)
    });
}

@end

};
