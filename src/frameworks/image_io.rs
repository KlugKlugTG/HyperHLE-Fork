/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `ImageIO.framework/ImageIO`.
//!
//! ImageIO is the framework for reading and writing images on iOS.
//! The app depends on this dylib; providing the HostDylib entry resolves
//! the linker warning and prevents non-lazy symbol references from
//! becoming NULL pointers.
//!
//! Per Apple documentation, ImageIO provides CGImageSource and
//! CGImageDestination APIs. If specific functions from this framework are
//! called by the guest in the future, they should be implemented here.

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/ImageIO.framework/ImageIO",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[],
    function_exports: &[],
};
