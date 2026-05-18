/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSKeyedArchiver` and serialization of its object graph format.
//!
//! Resources:
//!
//! - You can get a good intuitive grasp of how the format works just by staring
//!   at a pretty-print of a simple archive file from something that can parse
//!   plists, e.g. `plutil -p` or `println!("{:#?}", plist::Value::...);`.
//! - Apple's [Archives and Serializations Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Archiving/Articles/archives.html)

use std::collections::HashMap;
use std::io::Cursor;

use plist::{to_writer_binary, Dictionary, Uid, Value};

use crate::frameworks::foundation::ns_keyed_unarchiver::NSKeyedArchiveRootObjectKey;
use crate::frameworks::foundation::ns_string::{get_static_str, to_rust_string};
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize};
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};
use crate::Environment;

struct NSKeyedArchiverHostObject {
    plist: Dictionary,
    encoded_data: id, // NSData *
    current_key: Option<Uid>,
    /// map of id => Uid
    already_archived: HashMap<id, Uid>,
}
impl HostObject for NSKeyedArchiverHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSKeyedArchiver: NSCoder

+ (id)allocWithZone:(NSZonePtr)_zone {
    let mut plist = Dictionary::new();
    // Archives made by NSKeyedArchiver have the plist pre-populated with these
    plist.insert("$archiver".into(), "NSKeyedArchiver".into());
    // The $objects begins with nil, stored as the string "$null".
    plist.insert("$objects".into(), Value::Array(vec![Value::String("$null".into())]));
    plist.insert("$top".into(), Dictionary::new().into());
    plist.insert("$version".into(), 100000.into());
    // Map nil to the first element in the $objects array ("$null")
    let mut already_archived = HashMap::new();
    already_archived.insert(nil, Uid::new(0));
    env.objc.alloc_object(this, Box::new(NSKeyedArchiverHostObject {
        plist,
        encoded_data: nil,
        current_key: None,
        already_archived
    }), &mut env.mem)
}

+ (id)archivedDataWithRootObject:(id)root_object { // NSCoding *
    let key = get_static_str(env, NSKeyedArchiveRootObjectKey);
    let instance: id = msg_class![env; NSKeyedArchiver new];
    () = msg![env; instance encodeObject:root_object forKey:key];
    let data: id = msg![env; instance encodedData];
    let data: id = msg_class![env; NSData dataWithData:data];
    release(env, instance);
    data
}

+ (bool)archiveRootObject:(id)root_object // NSCoding *
                   toFile:(id)file { // NSString *
    log_dbg!("[NSKeyedArchiver archiveRootObject:{:?} toFile:{:?}('{}')]", root_object, file, to_rust_string(env, file));
    let data: id = msg![env; this archivedDataWithRootObject:root_object];
    msg![env; data writeToFile:file atomically:true]
}

- (id)initForWritingWithMutableData:(id)_data {
    log!("Warning: stubbed NSKeyedArchiver initForWritingWithMutableData:");
    this
}

// NSCoder protocol: NSKeyedArchiver always uses keyed coding.
// Per Apple docs: "Returns YES if the receiver supports keyed coding
// of objects, NO otherwise."
- (bool)allowsKeyedCoding {
    true
}

// encodeValueOfObjCType:at: — required by NSCoding conformers that use
// non-keyed encoding paths (e.g. NSNumber's fallback). According to Apple
// documentation, this method encodes a value of the given Objective-C type
// from the buffer at `addr`. For a keyed archiver, we store the raw bytes
// as data under a generated sequential key so the archive remains valid.
- (())encodeValueOfObjCType:(ConstPtr<u8>)type_ptr at:(ConstVoidPtr)addr {
    if type_ptr.is_null() || addr.is_null() {
        log!("Warning: encodeValueOfObjCType:at: called with null arguments");
        return;
    }
    // Read the ObjC type encoding character to determine the size
    let type_char: u8 = env.mem.read(type_ptr);
    let size: GuestUSize = match type_char {
        b'c' | b'C' | b'B' => 1,  // char, unsigned char, BOOL
        b's' | b'S' => 2,          // short, unsigned short
        b'i' | b'I' | b'l' | b'L' | b'f' => 4, // int, unsigned int, long, unsigned long, float
        b'q' | b'Q' | b'd' => 8,  // long long, unsigned long long, double
        _ => 4, // default to 4 bytes for unknown types (pointer-sized on 32-bit)
    };
    let data = env.mem.bytes_at(addr.cast(), size).to_vec();
    // Store as raw data in the current encoding scope under a type-prefixed key.
    // NSKeyedArchiver on real iOS uses "$0", "$1", ... for positional encoding;
    // we use a similar scheme with a type prefix for debuggability.
    let scope = get_value_to_encode_for_current_key(env, this);
    // Generate a unique positional key
    let mut idx = 0u32;
    loop {
        let key = format!("${}", idx);
        if !scope.contains_key(&key) {
            // Encode primitive values natively when possible for better
            // plist compatibility, fall back to Data for exotic types.
            let value: Value = match type_char {
                b'c' | b'C' | b'B' => (data[0] as i64).into(),
                b's' => (i16::from_le_bytes([data[0], data[1]]) as i64).into(),
                b'S' => (u16::from_le_bytes([data[0], data[1]]) as i64).into(),
                b'i' | b'l' => (i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64).into(),
                b'I' | b'L' => (u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64).into(),
                b'f' => (f32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64).into(),
                b'q' => i64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]).into(),
                b'd' => f64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]).into(),
                _ => Value::Data(data),
            };
            scope.insert(key, value);
            break;
        }
        idx += 1;
    }
}

- (())encodeObject:(id)object // NSCoding *
            forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    encode_object_for_key(env, this, object, key);
}

- (())encodeInt:(u32)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeInt32:(i32)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeInt64:(i64)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeInteger:(NSInteger)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeBool:(bool)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeFloat:(f32)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, (value as f64).into());
}

- (())encodeDouble:(f64)value forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, value.into());
}

- (())encodeBytes:(ConstPtr<u8>)bytes
           length:(NSUInteger)length
           forKey:(id)key { // NSString *
    let Some(key) = normalize_key(env, key) else { return; };
    let data = env.mem.bytes_at(bytes.cast(), length).to_vec();
    let scope = get_value_to_encode_for_current_key(env, this);
    if scope.contains_key(&key) {
        log!("Warning: NSKeyedArchiver overwriting existing value for key '{}'", key);
    }
    scope.insert(key, Value::Data(data));
}

- (())finishEncoding {
    let plist = &env.objc.borrow::<NSKeyedArchiverHostObject>(this).plist;
    let mut buffer = Vec::new();
    let cursor = Cursor::new(&mut buffer);
    to_writer_binary(cursor, plist).unwrap();
    let len = buffer.len() as GuestUSize;
    let guest_buffer = env.mem.alloc(len);
    env.mem.bytes_at_mut(guest_buffer.cast(), len).copy_from_slice(&buffer[..]);
    let encoded_data: id = msg_class![env;
    NSData dataWithBytesNoCopy:guest_buffer length:len];
    env.objc.borrow_mut::<NSKeyedArchiverHostObject>(this).encoded_data = encoded_data;
    retain(env, encoded_data);
}

- (id)encodedData {
    if env.objc.borrow::<NSKeyedArchiverHostObject>(this).encoded_data == nil {
        () = msg![env;
        this finishEncoding];
    }
    env.objc.borrow::<NSKeyedArchiverHostObject>(this).encoded_data
}

- (())dealloc {
    let NSKeyedArchiverHostObject { encoded_data, .. } = *env.objc.borrow::<NSKeyedArchiverHostObject>(this);
    release(env, encoded_data);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

};

fn normalize_key(env: &mut Environment, key: id) -> Option<String> {
    if key == nil {
        log!("Warning: NSKeyedArchiver received nil key. Ignoring to prevent crash.");
        return None;
    }

    let key_str = to_rust_string(env, key);

    if key_str.starts_with('$') {
        // Если ключ начинается с $, добавляем еще один $, чтобы избежать конфликтов с системными ключами
        log!(
            "Warning: NSKeyedArchiver mangling key starting with '$': {}",
            key_str
        );
        Some(format!("${}", key_str))
    } else {
        // Конвертируем Cow в String
        Some(key_str.into_owned())
    }
}

fn get_value_to_encode_for_current_key(env: &mut Environment, archiver: id) -> &mut Dictionary {
    assert_eq!(
        env.objc
            .borrow::<NSKeyedArchiverHostObject>(archiver)
            .encoded_data,
        nil
    );
    let host_object = env.objc.borrow_mut::<NSKeyedArchiverHostObject>(archiver);
    match host_object.current_key {
        Some(uid) => host_object
            .plist
            .get_mut("$objects")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .get_mut(uid.get() as usize)
            .unwrap(),
        None => host_object.plist.get_mut("$top").unwrap(),
    }
    .as_dictionary_mut()
    .unwrap()
}

fn encode_object(env: &mut Environment, archiver: id, object: id) -> Uid {
    let class = msg![env; object class];
    let host_object = env.objc.borrow_mut::<NSKeyedArchiverHostObject>(archiver);
    if let Some(existing_uid) = host_object.already_archived.get(&object).cloned() {
        // Object has already been archived, just insert a UID reference
        existing_uid
    } else {
        // Object has not been archived yet, encode it and insert reference
        host_object
            .plist
            .get_mut("$objects")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(Dictionary::new().into());
        let len = host_object.plist["$objects"].as_array().unwrap().len();
        let new_uid = Uid::new(len as u64 - 1);
        if object == class {
            // If the class selector returns itself, we're encoding a Class
            let classname = Value::String(env.objc.get_class_name(class).into());
            let mut classes = Vec::new();
            let mut current_class = class;
            while current_class != nil {
                let class_name = env.objc.get_class_name(current_class);
                classes.push(Value::String(class_name.into()));
                current_class = env.objc.get_superclass(current_class);
            }
            let host_object = env.objc.borrow_mut::<NSKeyedArchiverHostObject>(archiver);
            let entry = host_object
                .plist
                .get_mut("$objects")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .get_mut(new_uid.get() as usize)
                .unwrap()
                .as_dictionary_mut()
                .unwrap();
            entry.insert("$classes".into(), Value::Array(classes));
            entry.insert("$classname".into(), classname);
        } else {
            // Register the object BEFORE encoding to break infinite recursion.
            // If encodeWithCoder: triggers another encode of the same object,
            // we'll find it in already_archived and return the existing UID.
            env.objc
                .borrow_mut::<NSKeyedArchiverHostObject>(archiver)
                .already_archived
                .insert(object, new_uid);

            let previous_key = env
                .objc
                .borrow_mut::<NSKeyedArchiverHostObject>(archiver)
                .current_key
                .replace(new_uid);
            let class: id = msg![env; object class];
            encode_object_for_key(env, archiver, class, "$class".into());
            () = msg![env; object encodeWithCoder:archiver];
            env.objc
                .borrow_mut::<NSKeyedArchiverHostObject>(archiver)
                .current_key = previous_key;
        }

        new_uid
    }
}

fn encode_object_for_key(env: &mut Environment, archiver: id, object: id, normalized_key: String) {
    let uid = encode_object(env, archiver, object);
    let scope = get_value_to_encode_for_current_key(env, archiver);

    if scope.contains_key(&normalized_key) {
        log!(
            "Warning: NSKeyedArchiver overwriting existing object for key '{}'",
            normalized_key
        );
    }

    scope.insert(normalized_key, Value::Uid(uid));
}
