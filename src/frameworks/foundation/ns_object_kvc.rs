use crate::Environment;
use crate::frameworks::core_animation::ca_transform3d::CATransform3D;
use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::NSUInteger;
use crate::mem::{SafeRead, guest_size_of};
use crate::objc::{Class, SEL, class_getInstanceMethod, id, method_getTypeEncoding, msg, msg_send};

fn skip_objc_type(encoding: &[u8], offset: &mut usize) -> Option<u8> {
    while encoding
        .get(*offset)
        .is_some_and(|byte| b"rnNoORV".contains(byte))
    {
        *offset += 1;
    }

    let kind = *encoding.get(*offset)?;
    *offset += 1;
    match kind {
        b'@' if encoding.get(*offset) == Some(&b'?') => *offset += 1,
        b'@' if encoding.get(*offset) == Some(&b'"') => {
            *offset += 1;
            while encoding.get(*offset).is_some_and(|byte| *byte != b'"') {
                *offset += 1;
            }
            if encoding.get(*offset) == Some(&b'"') {
                *offset += 1;
            }
        }
        b'^' => {
            skip_objc_type(encoding, offset)?;
        }
        b'[' => {
            while encoding.get(*offset).is_some_and(u8::is_ascii_digit) {
                *offset += 1;
            }
            skip_objc_type(encoding, offset)?;
            while encoding.get(*offset).is_some_and(u8::is_ascii_digit) {
                *offset += 1;
            }
            if encoding.get(*offset) == Some(&b']') {
                *offset += 1;
            }
        }
        b'{' | b'(' => {
            let close = if kind == b'{' { b'}' } else { b')' };
            while let Some(byte) = encoding.get(*offset) {
                if *byte == b'=' || *byte == close {
                    break;
                }
                *offset += 1;
            }
            if encoding.get(*offset) == Some(&b'=') {
                *offset += 1;
                while encoding.get(*offset).is_some_and(|byte| *byte != close) {
                    if encoding.get(*offset) == Some(&b'"') {
                        *offset += 1;
                        while encoding.get(*offset).is_some_and(|byte| *byte != b'"') {
                            *offset += 1;
                        }
                        if encoding.get(*offset) == Some(&b'"') {
                            *offset += 1;
                        }
                    }
                    skip_objc_type(encoding, offset)?;
                }
            }
            if encoding.get(*offset) == Some(&close) {
                *offset += 1;
            }
        }
        b'b' => {
            while encoding.get(*offset).is_some_and(u8::is_ascii_digit) {
                *offset += 1;
            }
        }
        _ => {}
    }
    Some(kind)
}

pub(super) fn objc_argument_encoding(encoding: &[u8], argument_index: usize) -> Option<&[u8]> {
    let mut offset = 0;
    skip_objc_type(encoding, &mut offset)?;
    skip_method_offset(encoding, &mut offset);
    for index in 0..=argument_index {
        let start = offset;
        skip_objc_type(encoding, &mut offset)?;
        if index == argument_index {
            return Some(&encoding[start..offset]);
        }
        skip_method_offset(encoding, &mut offset);
    }
    None
}

pub(super) fn objc_argument_type(encoding: &[u8], argument_index: usize) -> Option<u8> {
    objc_argument_encoding(encoding, argument_index)?
        .first()
        .copied()
}

fn skip_method_offset(encoding: &[u8], offset: &mut usize) {
    if encoding
        .get(*offset)
        .is_some_and(|byte| matches!(*byte, b'+' | b'-'))
    {
        *offset += 1;
    }
    while encoding.get(*offset).is_some_and(u8::is_ascii_digit) {
        *offset += 1;
    }
}

pub(super) fn kvc_setter_argument_encoding(
    env: &mut Environment,
    class: Class,
    selector: SEL,
) -> Option<Vec<u8>> {
    let method = class_getInstanceMethod(env, class, selector);
    if method.is_null() {
        return None;
    }
    let encoding = method_getTypeEncoding(env, method);
    if encoding.is_null() {
        return None;
    }
    let encoding = env.mem.cstr_at(encoding.cast()).to_vec();
    objc_argument_encoding(&encoding, 2).map(<[u8]>::to_vec)
}

pub(super) fn kvc_setter_argument_type(
    env: &mut Environment,
    class: Class,
    selector: SEL,
) -> Option<u8> {
    kvc_setter_argument_encoding(env, class, selector)?
        .first()
        .copied()
}

pub(super) fn send_numeric_kvc_setter(
    env: &mut Environment,
    object: id,
    selector: SEL,
    value: id,
    argument_type: u8,
) -> bool {
    match argument_type {
        b'B' => {
            let value: bool = msg![env; value boolValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'c' => {
            let value: i8 = msg![env; value charValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'C' => {
            let value: u8 = msg![env; value unsignedCharValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b's' => {
            let value: i16 = msg![env; value shortValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'S' => {
            let value: u16 = msg![env; value unsignedShortValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'i' => {
            let value: i32 = msg![env; value intValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'I' => {
            let value: u32 = msg![env; value unsignedIntValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'l' => {
            let value: i32 = msg![env; value longValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'L' => {
            let value: NSUInteger = msg![env; value unsignedLongValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'q' => {
            let value: i64 = msg![env; value longLongValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'Q' => {
            let value: u64 = msg![env; value unsignedLongLongValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'f' => {
            let value: f32 = msg![env; value floatValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b'd' => {
            let value: f64 = msg![env; value doubleValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        _ => return false,
    }
    true
}

pub(super) fn send_nsvalue_kvc_setter(
    env: &mut Environment,
    object: id,
    selector: SEL,
    value: id,
    argument_encoding: &[u8],
) -> bool {
    match argument_encoding {
        b"{CGPoint=ff}" => {
            let value: CGPoint = msg![env; value CGPointValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b"{CGSize=ff}" => {
            let value: CGSize = msg![env; value CGSizeValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b"{CGRect={CGPoint=ff}{CGSize=ff}}" => {
            let value: CGRect = msg![env; value CGRectValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        b"{CGAffineTransform=ffffff}" => {
            let value: CGAffineTransform = read_nsvalue(env, value);
            let _: () = msg_send(env, (object, selector, value));
        }
        b"{CATransform3D=ffffffffffffffff}" => {
            let value: CATransform3D = msg![env; value CATransform3DValue];
            let _: () = msg_send(env, (object, selector, value));
        }
        _ => return false,
    }
    true
}

pub(super) fn send_kvc_setter(
    env: &mut Environment,
    class: Class,
    object: id,
    selector: SEL,
    value: id,
    is_number: bool,
    is_value: bool,
) -> bool {
    let Some(encoding) = kvc_setter_argument_encoding(env, class, selector) else {
        return false;
    };
    if is_number {
        if let Some(argument_type) = encoding.first().copied() {
            if send_numeric_kvc_setter(env, object, selector, value, argument_type) {
                return true;
            }
        }
        return false;
    }
    is_value && send_nsvalue_kvc_setter(env, object, selector, value, &encoding)
}

fn read_nsvalue<T: SafeRead>(env: &mut Environment, value: id) -> T {
    let buffer = env.mem.alloc(guest_size_of::<T>());
    let _: () = msg![env; value getValue:buffer];
    let result = env.mem.read(buffer.cast());
    env.mem.free(buffer);
    result
}
