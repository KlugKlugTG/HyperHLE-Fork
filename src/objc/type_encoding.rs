/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Parsing of Objective-C method type encodings (e.g. `v12@0:4f8`).
//!
//! Resources:
//! - [Type encodings](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtTypeEncodings.html)
//! - Apple's objc4 `objc-typeencoding.mm` (`SkipFirstType`,
//!   `encoding_getNumberOfArguments`, `encoding_copyArgumentType`, ...),
//!   whose behaviour the helpers here mirror.

/// Skip one complete type (including leading type qualifiers such as
/// `r`/`n`/`o`/`V`) starting at `*offset`. Returns the type's "kind"
/// character (e.g. `b'@'`, `b'{'`), or `None` at the end of the string.
pub(crate) fn skip_type(encoding: &[u8], offset: &mut usize) -> Option<u8> {
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
            skip_type(encoding, offset)?;
        }
        b'[' => {
            while encoding.get(*offset).is_some_and(u8::is_ascii_digit) {
                *offset += 1;
            }
            skip_type(encoding, offset)?;
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
                    skip_type(encoding, offset)?;
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

/// Skip the (optional, possibly signed) stack offset that follows each type
/// in a method encoding, plus the GNU runtime's `+` register hint.
pub(crate) fn skip_offset(encoding: &[u8], offset: &mut usize) {
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

/// The return type of a method encoding, without its stack-size suffix.
pub(crate) fn method_return_type(encoding: &[u8]) -> Option<&[u8]> {
    let mut offset = 0;
    skip_type(encoding, &mut offset)?;
    Some(&encoding[..offset])
}

/// The type of argument `index` (0 = `self`, 1 = `_cmd`) of a method
/// encoding, without its stack-offset suffix.
pub(crate) fn method_argument_type(encoding: &[u8], index: usize) -> Option<&[u8]> {
    let mut offset = 0;
    skip_type(encoding, &mut offset)?;
    skip_offset(encoding, &mut offset);
    for current in 0..=index {
        let start = offset;
        skip_type(encoding, &mut offset)?;
        if current == index {
            return Some(&encoding[start..offset]);
        }
        skip_offset(encoding, &mut offset);
    }
    None
}

/// Number of arguments (including `self` and `_cmd`) described by a method
/// encoding. Equivalent to objc4's `encoding_getNumberOfArguments`.
pub(crate) fn method_argument_count(encoding: &[u8]) -> usize {
    let mut offset = 0;
    if skip_type(encoding, &mut offset).is_none() {
        return 0;
    }
    skip_offset(encoding, &mut offset);
    let mut count = 0;
    while offset < encoding.len() {
        let before = offset;
        if skip_type(encoding, &mut offset).is_none() {
            break;
        }
        skip_offset(encoding, &mut offset);
        count += 1;
        if offset == before {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_arguments() {
        assert_eq!(method_argument_count(b"v8@0:4"), 2);
        assert_eq!(method_argument_count(b"v12@0:4f8"), 3);
        assert_eq!(method_argument_count(b"v@:"), 2);
        assert_eq!(method_argument_count(b"@20@0:4@8^{_NSZone=}12i16"), 5);
        assert_eq!(
            method_argument_count(b"{CGRect={CGPoint=ff}{CGSize=ff}}16@0:4@\"NSNumber\"8"),
            3
        );
        assert_eq!(method_argument_count(b"v16@0:4@?8r*12"), 4);
        assert_eq!(method_argument_count(b"v-4@0:4"), 2);
        assert_eq!(method_argument_count(b""), 0);
    }

    #[test]
    fn extracts_types() {
        let enc = b"{CGSize=ff}20@0:4Vr^{CGPoint=ff}8[4i]12";
        assert_eq!(method_return_type(enc), Some(&b"{CGSize=ff}"[..]));
        assert_eq!(method_argument_type(enc, 0), Some(&b"@"[..]));
        assert_eq!(method_argument_type(enc, 1), Some(&b":"[..]));
        assert_eq!(method_argument_type(enc, 2), Some(&b"Vr^{CGPoint=ff}"[..]));
        assert_eq!(method_argument_type(enc, 3), Some(&b"[4i]"[..]));
        assert_eq!(method_argument_type(enc, 4), None);
    }
}
