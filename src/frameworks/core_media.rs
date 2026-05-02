/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stub for `CoreMedia.framework/CoreMedia`.
//!
//! On iOS, CoreMedia provides time-related types (`CMTime`, `CMTimeRange`),
//! sample buffer plumbing (`CMSampleBufferRef`), and format descriptions used
//! mostly by AVFoundation. Apps that link against CoreMedia (directly or
//! transitively, e.g. via AVFoundation cutscene playback) put the path
//! `/System/Library/Frameworks/CoreMedia.framework/CoreMedia` in their Mach-O
//! load commands.
//!
//! Without a [crate::dyld::HostDylib] entry for that path, HyperHLE prints a
//! `Warning: app binary depends on unimplemented or missing dylib
//! "/System/Library/Frameworks/CoreMedia.framework/CoreMedia"` at startup,
//! which can spook users into reporting otherwise-fine apps as broken (e.g.
//! HyperHLE appdb report #22, GhostToasters).
//!
//! This stub exists so that the dependency is recognized and the warning is
//! suppressed. The few CoreMedia functions HyperHLE currently implements
//! (`CMSampleBufferGetImageBuffer`, `CMSampleBufferDataIsReady`, …) are
//! registered with [crate::frameworks::core_video] for historical reasons;
//! `dyld` searches all framework `function_exports` regardless of which
//! dylib they were declared under, so the binding still resolves correctly
//! whether the app links CoreMedia or CoreVideo.

use crate::dyld::FunctionExports;

pub const FUNCTIONS: FunctionExports = &[];
