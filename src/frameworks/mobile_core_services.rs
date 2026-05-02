/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stub for `MobileCoreServices.framework/MobileCoreServices`.
//!
//! On iOS, MobileCoreServices is the umbrella for Uniform Type Identifier
//! types and constants (`UTType*`, `kUTType*`). It is pulled in transitively
//! by all sorts of high-level UIKit APIs — `UIDocumentInteractionController`,
//! `MFMailComposeViewController`, `UIImagePickerController` — so most apps
//! end up listing it in their Mach-O dependency table even when they never
//! call any UTType function directly.
//!
//! Without a [crate::dyld::HostDylib] entry for the path
//! `/System/Library/Frameworks/MobileCoreServices.framework/MobileCoreServices`,
//! HyperHLE prints a `Warning: app binary depends on unimplemented or missing
//! dylib …` at startup, which can spook users into reporting otherwise-fine
//! apps as broken (e.g. HyperHLE appdb report #23, Mutant Fridge).
//!
//! This stub exists so the dependency is recognized and the warning is
//! suppressed. Real UTType handling is not implemented; functions that
//! actually need to consult UTType data should be added here as they come up.

use crate::dyld::FunctionExports;

pub const FUNCTIONS: FunctionExports = &[];
