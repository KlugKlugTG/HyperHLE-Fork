/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `GLKit.framework/GLKit`.
//!
//! GLKit provides math utilities and helpers for OpenGL ES apps.
//! The app depends on this dylib and references `GLKMatrix4Identity`.
//!
//! Per Apple documentation (`GLKMatrix4`):
//! `GLKMatrix4Identity` is a constant identity matrix (16 floats, column-major).

use crate::dyld::{ConstantExports, HostConstant};
use crate::mem::ConstVoidPtr;
use crate::Environment;

/// Allocate guest memory for `GLKMatrix4Identity` and return its pointer.
/// The symbol `_GLKMatrix4Identity` is exported as a `const GLKMatrix4`.
/// In touchHLE the linker writes the address returned by this custom
/// initializer into the non-lazy pointer slot.
fn make_glk_matrix4_identity(env: &mut Environment) -> ConstVoidPtr {
    // Column-major identity matrix (16 floats).
    let identity: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    let ptr = env.mem.alloc((identity.len() * 4) as u32);
    for (i, &v) in identity.iter().enumerate() {
        let p: crate::mem::MutPtr<f32> = ptr.cast::<f32>() + i as u32;
        env.mem.write(p, v);
    }
    ptr.cast_const().cast()
}

pub const CONSTANTS: ConstantExports = &[
    (
        "_GLKMatrix4Identity",
        HostConstant::Custom(make_glk_matrix4_identity),
    ),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/GLKit.framework/GLKit",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[],
};
