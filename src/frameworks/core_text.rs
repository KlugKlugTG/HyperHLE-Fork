/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stub for `CoreText.framework/CoreText`.
//!
//! On iOS, CoreText is the low-level text-layout and font API (`CTFontRef`,
//! `CTFramesetterRef`, `CTLineRef`, `CTRunRef`, `CFAttributedStringRef`-based
//! drawing). It's pulled in transitively by lots of higher-level code paths —
//! anything that ends up calling `CGContextShowTextAtPoint` with a custom
//! font, third-party text-rendering libraries, custom `UILabel` subclasses,
//! etc. — so apps frequently list it in their Mach-O dependency table even
//! when they never call any `CT*` function directly.
//!
//! Without a [crate::dyld::HostDylib] entry for the path
//! `/System/Library/Frameworks/CoreText.framework/CoreText`, touchHLE prints
//! `Warning: app binary depends on unimplemented or missing dylib …` at
//! startup, which can spook users into reporting otherwise-fine apps as
//! broken (e.g. HyperHLE appdb report #32, PopStar!).
//!
//! This stub exists so the dependency is recognized and the warning is
//! suppressed. Real CoreText layout is not implemented; functions that
//! actually need to render text via CT* APIs should be added here as they
//! come up.

use crate::dyld::FunctionExports;

pub const FUNCTIONS: FunctionExports = &[];
