/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CoreText.framework/CoreText`
//!
//! CoreText is the C-level text rendering API exposed by macOS and iOS
//! since iOS 3.2. We don't currently implement layout/glyph metrics
//! (apps that need full text shaping fall back to UIKit/UIFont), but
//! many apps reference the framework's exported string constants —
//! either as attribute keys when constructing font descriptor
//! dictionaries (`CTFontDescriptorCreateWithAttributes`) or as
//! attribute keys when building `CFAttributedStringRef` /
//! `NSAttributedString` instances for `CTFramesetterCreateWithAttributedString`
//! and friends — purely for `isEqual:` comparisons against keys
//! returned by the system.
//!
//! Per Apple's `CTFontDescriptor.h`, `CTFont.h`, `CTFontTraits.h` and
//! `CTStringAttributes.h` headers these are `CFStringRef` constants
//! with a canonical string value. For attributed-string attribute
//! keys most of them have the same value as their `NSAttributedString`
//! counterpart (e.g. `kCTFontAttributeName` == the `CFStringRef` for
//! the C string "NSFont", which is the same as the AppKit / UIKit
//! `NSFontAttributeName`), which is what makes CoreText / Foundation
//! attributed strings toll-free bridgeable. For touchHLE's purposes
//! the exact textual content only matters for identity comparisons;
//! we mirror the spelling Apple's public headers document.
//!
//! References:
//! - <https://developer.apple.com/documentation/coretext/font_descriptor_attribute_keys>
//! - <https://developer.apple.com/documentation/coretext/core_text_string_attributes>
//! - `CTFontDescriptor.h`, `CTFont.h`, `CTFontTraits.h`,
//!   `CTStringAttributes.h` (Apple SDK).

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant, HostDylib};
use crate::frameworks::core_graphics::cg_bitmap_context::CGBitmapContextDrawer;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::core_graphics::cg_font::{CGFontCreateWithFontName, CGFontRef, CGGlyph};
use crate::frameworks::core_foundation::CFRange;
use crate::frameworks::foundation::ns_string::{get_static_str, to_rust_string};
use crate::frameworks::foundation::{unichar, NSRange, NSUInteger};
use crate::frameworks::uikit::ui_font::{font_from_uifont, is_uifont, draw_font_glyph};
use crate::font::{Font, TextAlignment, WrapMode};
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, Ptr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, retain, ClassExports, HostObject,
};
use crate::Environment;

/// Opaque CoreText font reference.
pub type CTFontRef = crate::objc::id;

/// Opaque CoreText font descriptor reference.
pub type CTFontDescriptorRef = crate::objc::id;

/// Opaque CoreText attributed string reference.
pub type CFAttributedStringRef = crate::objc::id;

/// Opaque CoreText typesetter reference.
pub type CTTypesetterRef = crate::objc::id;

/// Opaque CoreText line reference.
pub type CTLineRef = crate::objc::id;

/// Opaque CoreText framesetter reference.
pub type CTFramesetterRef = crate::objc::id;

/// Opaque CoreText frame reference.
pub type CTFrameRef = crate::objc::id;

#[derive(Clone, Default)]
struct CTFontDescriptorHostObject {
    attrs: id,
}
impl HostObject for CTFontDescriptorHostObject {}

#[derive(Clone, Default)]
struct CTFontHostObject {
    font: id,
}
impl HostObject for CTFontHostObject {}

#[derive(Clone, Default)]
struct CTAttributedStringHostObject {
    string: id,
    attrs: id,
}
impl HostObject for CTAttributedStringHostObject {}

#[derive(Clone, Default)]
struct CTTypesetterHostObject {
    attr_string: id,
}
impl HostObject for CTTypesetterHostObject {}

#[derive(Clone, Default)]
struct CTLineHostObject {
    text: id,
    font: id,
}
impl HostObject for CTLineHostObject {}

#[derive(Clone, Default)]
struct CTFramesetterHostObject {
    attr_string: id,
}
impl HostObject for CTFramesetterHostObject {}

#[derive(Clone, Default)]
struct CTFrameHostObject {
    /// The single CTLine we produce for the (whole) attributed string. Our
    /// layout engine doesn't break a framesetter's string across multiple
    /// lines/columns, so a frame holds exactly one line. This matches what
    /// the apps in the corpus need (they mostly measure / draw short labels).
    line: id,
    /// The string range covered by the frame, mirrored back from the
    /// attributed string for `CTFrameGetStringRange`/`GetVisibleStringRange`.
    range: CFRange,
}
impl HostObject for CTFrameHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_CTFontDescriptor: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CTFont: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CFAttributedString: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CTTypesetter: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CTLine: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CTFramesetter: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

@implementation _touchHLE_CTFrame: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

};

fn alloc_descriptor(env: &mut Environment, attrs: id) -> CTFontDescriptorRef {
    let class = env.objc.get_known_class("_touchHLE_CTFontDescriptor", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTFontDescriptorHostObject { attrs }), &mut env.mem)
}

fn alloc_font(env: &mut Environment, font: id) -> CTFontRef {
    let class = env.objc.get_known_class("_touchHLE_CTFont", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTFontHostObject { font }), &mut env.mem)
}

fn alloc_attr_string(env: &mut Environment, string: id, attrs: id) -> CFAttributedStringRef {
    let class = env.objc.get_known_class("_touchHLE_CFAttributedString", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTAttributedStringHostObject { string, attrs }), &mut env.mem)
}

fn alloc_typesetter(env: &mut Environment, attr_string: id) -> CTTypesetterRef {
    let class = env.objc.get_known_class("_touchHLE_CTTypesetter", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTTypesetterHostObject { attr_string }), &mut env.mem)
}

fn alloc_line(env: &mut Environment, text: id, font: id) -> CTLineRef {
    let class = env.objc.get_known_class("_touchHLE_CTLine", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTLineHostObject { text, font }), &mut env.mem)
}

fn alloc_framesetter(env: &mut Environment, attr_string: id) -> CTFramesetterRef {
    let class = env.objc.get_known_class("_touchHLE_CTFramesetter", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTFramesetterHostObject { attr_string }), &mut env.mem)
}

fn alloc_frame(env: &mut Environment, line: id, range: CFRange) -> CTFrameRef {
    let class = env.objc.get_known_class("_touchHLE_CTFrame", &mut env.mem);
    env.objc.alloc_object(class, Box::new(CTFrameHostObject { line, range }), &mut env.mem)
}

fn descriptor_attrs(env: &mut Environment, descriptor: CTFontDescriptorRef) -> id {
    env.objc.borrow::<CTFontDescriptorHostObject>(descriptor).attrs
}

fn font_from_descriptor(env: &mut Environment, descriptor: CTFontDescriptorRef, size: CGFloat) -> id {
    let attrs = descriptor_attrs(env, descriptor);
    if attrs == nil {
        return msg_class![env; UIFont systemFontOfSize:size];
    }
    let font_name_key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSFontNameAttribute");
    let family_key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSFontFamilyAttribute");
    let size_key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSFontSizeAttribute");
    let mut name: id = nil;
    let by_name: id = msg![env; attrs objectForKey:font_name_key];
    if by_name != nil {
        name = by_name;
    } else {
        let family: id = msg![env; attrs objectForKey:family_key];
        if family != nil {
            let family_str = to_rust_string(env, family).into_owned();
            name = match family_str.as_str() {
                "Courier New" => get_static_str(env, "CourierNewPSMT"),
                "Helvetica" => get_static_str(env, "Helvetica"),
                "Times New Roman" => get_static_str(env, "TimesNewRomanPSMT"),
                _ => nil,
            };
        }
    }
    let font_size: CGFloat = if size > 0.0 {
        size
    } else {
        let size_num: id = msg![env; attrs objectForKey:size_key];
        if size_num != nil {
            msg![env; size_num floatValue]
        } else {
            12.0
        }
    };
    if name == nil {
        msg_class![env; UIFont systemFontOfSize:font_size]
    } else {
        msg_class![env; UIFont fontWithName:name size:font_size]
    }
}

fn attributed_string_text(env: &mut Environment, attr_string: CFAttributedStringRef) -> id {
    env.objc.borrow::<CTAttributedStringHostObject>(attr_string).string
}

fn attributed_string_attrs(env: &mut Environment, attr_string: CFAttributedStringRef) -> id {
    env.objc.borrow::<CTAttributedStringHostObject>(attr_string).attrs
}

fn string_and_font_from_attr_string(env: &mut Environment, attr_string: CFAttributedStringRef) -> (id, id) {
    let string = attributed_string_text(env, attr_string);
    let attrs = attributed_string_attrs(env, attr_string);
    let font = if attrs == nil {
        msg_class![env; UIFont systemFontOfSize:12.0]
    } else {
        let key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSFont");
        let maybe_font = msg![env; attrs objectForKey:key];
        if maybe_font != nil {
            if is_uifont(env, maybe_font) {
                maybe_font
            } else {
                msg_class![env; UIFont systemFontOfSize:12.0]
            }
        } else {
            msg_class![env; UIFont systemFontOfSize:12.0]
        }
    };
    (string, font)
}

fn CTFontDescriptorCreateWithAttributes(env: &mut Environment, attributes: id) -> CTFontDescriptorRef {
    let attrs = if attributes == nil {
        msg_class![env; NSDictionary dictionary]
    } else {
        attributes
    };
    retain(env, attrs);
    alloc_descriptor(env, attrs)
}

fn CTFontCreateWithFontDescriptor(
    env: &mut Environment,
    descriptor: CTFontDescriptorRef,
    size: CGFloat,
    _matrix: ConstVoidPtr,
) -> CTFontRef {
    let font = font_from_descriptor(env, descriptor, size);
    retain(env, font);
    alloc_font(env, font)
}

fn CTFontGetSize(env: &mut Environment, font: CTFontRef) -> CGFloat {
    if font.is_null() {
        return 0.0;
    }
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    if ui_font == nil {
        return 0.0;
    }
    msg![env; ui_font pointSize]
}

fn CTFontGetAscent(env: &mut Environment, font: CTFontRef) -> CGFloat {
    if font.is_null() { return 0.0; }
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    if ui_font == nil { return 0.0; }
    msg![env; ui_font ascender]
}

fn CTFontGetDescent(env: &mut Environment, font: CTFontRef) -> CGFloat {
    if font.is_null() { return 0.0; }
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    if ui_font == nil { return 0.0; }
    -msg![env; ui_font descender]
}

fn CTFontGetLeading(env: &mut Environment, font: CTFontRef) -> CGFloat {
    if font.is_null() { return 0.0; }
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    if ui_font == nil { return 0.0; }
    msg![env; ui_font leading]
}

fn CFAttributedStringCreate(
    env: &mut Environment,
    _allocator: crate::frameworks::core_foundation::cf_allocator::CFAllocatorRef,
    string: id,
    attributes: id,
) -> CFAttributedStringRef {
    let s = if string == nil { msg_class![env; NSString string] } else { string };
    let attrs = if attributes == nil { msg_class![env; NSDictionary dictionary] } else { attributes };
    retain(env, s);
    retain(env, attrs);
    alloc_attr_string(env, s, attrs)
}

fn CFAttributedStringCreateCopy(
    env: &mut Environment,
    _allocator: crate::frameworks::core_foundation::cf_allocator::CFAllocatorRef,
    string: CFAttributedStringRef,
) -> CFAttributedStringRef {
    if string.is_null() { return nil; }
    let host = env.objc.borrow::<CTAttributedStringHostObject>(string).clone();
    retain(env, host.string);
    retain(env, host.attrs);
    alloc_attr_string(env, host.string, host.attrs)
}

fn CFAttributedStringGetString(env: &mut Environment, attr_string: CFAttributedStringRef) -> id {
    if attr_string.is_null() { return nil; }
    env.objc.borrow::<CTAttributedStringHostObject>(attr_string).string
}

fn CTTypesetterCreateWithAttributedString(env: &mut Environment, string: CFAttributedStringRef) -> CTTypesetterRef {
    if string.is_null() { return nil; }
    retain(env, string);
    alloc_typesetter(env, string)
}

fn CTTypesetterCreateLine(env: &mut Environment, typesetter: CTTypesetterRef, _range: NSRange) -> CTLineRef {
    if typesetter.is_null() { return nil; }
    let attr_string = env.objc.borrow::<CTTypesetterHostObject>(typesetter).attr_string;
    let (text, font) = string_and_font_from_attr_string(env, attr_string);
    retain(env, text);
    retain(env, font);
    alloc_line(env, text, font)
}

fn CTLineCreateWithAttributedString(env: &mut Environment, string: CFAttributedStringRef) -> CTLineRef {
    if string.is_null() { return nil; }
    let (text, font) = string_and_font_from_attr_string(env, string);
    retain(env, text);
    retain(env, font);
    alloc_line(env, text, font)
}

fn line_text(env: &mut Environment, line: CTLineRef) -> id {
    env.objc.borrow::<CTLineHostObject>(line).text
}

fn line_font(env: &mut Environment, line: CTLineRef) -> id {
    env.objc.borrow::<CTLineHostObject>(line).font
}

fn CTLineGetTypographicBounds(
    env: &mut Environment,
    line: CTLineRef,
    ascent: MutPtr<CGFloat>,
    descent: MutPtr<CGFloat>,
    leading: MutPtr<CGFloat>,
) -> CGFloat {
    if line.is_null() {
        return 0.0;
    }
    let text = line_text(env, line);
    let font = line_font(env, line);
    let size: CGFloat = msg![env; font pointSize];
    let ascent_val: CGFloat = msg![env; font ascender];
    let descender_val: CGFloat = msg![env; font descender];
    let leading_val: CGFloat = msg![env; font leading];
    let text_str = to_rust_string(env, text).into_owned();
    let font_obj = font_from_uifont(env, font).unwrap_or_else(Font::sans_regular);
    let width = font_obj.calculate_text_size(size, &text_str, None).0;
    if !ascent.is_null() { env.mem.write(ascent, ascent_val); }
    if !descent.is_null() { env.mem.write(descent, -descender_val); }
    if !leading.is_null() { env.mem.write(leading, leading_val); }
    width
}

fn CTLineGetImageBounds(env: &mut Environment, line: CTLineRef, _context: id) -> CGRect {
    if line.is_null() {
        return CGRect::default();
    }
    let text = line_text(env, line);
    let font = line_font(env, line);
    let size: CGFloat = msg![env; font pointSize];
    let ascent_val: CGFloat = msg![env; font ascender];
    let text_str = to_rust_string(env, text).into_owned();
    let font_obj = font_from_uifont(env, font).unwrap_or_else(Font::sans_regular);
    let (w, h) = font_obj.calculate_text_size(size, &text_str, None);
    CGRect { origin: CGPoint { x: 0.0, y: -ascent_val }, size: CGSize { width: w, height: h } }
}

fn CTLineDraw(env: &mut Environment, line: CTLineRef, context: id) {
    if line.is_null() || context.is_null() { return; }
    let text = line_text(env, line);
    let font = line_font(env, line);
    let size: CGFloat = msg![env; font pointSize];
    let text_str = to_rust_string(env, text).into_owned();
    let font_obj = font_from_uifont(env, font).unwrap_or_else(Font::sans_regular);
    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    let fill_color = drawer.rgb_fill_color();
    font_obj.draw(size, &text_str, (0.0, 0.0), None, TextAlignment::Left, |glyph| {
        draw_font_glyph(&mut drawer, glyph, fill_color, None, None)
    });
}

/// `CFIndex CTLineGetGlyphCount(CTLineRef line)`
///
/// Returns the total glyph count for the line. We map the line's text to
/// glyphs through the backing font's cmap (one glyph per UTF-16 code unit,
/// which matches CoreText for the simple Latin/Cyrillic text these apps lay
/// out), so callers that size buffers off this count get a usable value.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509609-ctlinegetglyphcount>
fn CTLineGetGlyphCount(env: &mut Environment, line: CTLineRef) -> i32 {
    if line.is_null() {
        return 0;
    }
    let text = line_text(env, line);
    if text == nil {
        return 0;
    }
    let len: NSUInteger = msg![env; text length];
    len as i32
}

/// `CFArrayRef CTLineGetGlyphRuns(CTLineRef line)`
///
/// Returns the array of glyph runs that make up the line. touchHLE lays text
/// out as a single run, so we return a one-element array wrapping the line
/// itself (our `_touchHLE_CTLine` host object also stands in for a run — the
/// run-level getters used by the apps in the corpus, e.g. glyph count, work
/// the same way). Returning a real (empty-safe) `CFArray` rather than NULL
/// prevents callers from dereferencing a NULL array.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509596-ctlinegetglyphruns>
fn CTLineGetGlyphRuns(env: &mut Environment, line: CTLineRef) -> id {
    if line.is_null() {
        return nil;
    }
    let arr: id = msg_class![env; NSMutableArray arrayWithCapacity:1u32];
    () = msg![env; arr addObject:line];
    let immutable: id = msg![env; arr copy];
    autorelease(env, immutable)
}

/// `bool CTFontGetGlyphsForCharacters(CTFontRef font, const UniChar characters[],
///     CGGlyph glyphs[], CFIndex count)`
///
/// Provides basic Unicode-to-glyph mapping for a font, writing one `CGGlyph`
/// per input `UniChar` via the backing font's cmap. Returns `true` only if
/// every character mapped to a non-zero (non-`.notdef`) glyph, exactly as
/// Apple documents.
///
/// Reference: <https://developer.apple.com/documentation/coretext/ctfontgetglyphsforcharacters(_:_:_:_:)>
fn CTFontGetGlyphsForCharacters(
    env: &mut Environment,
    font: CTFontRef,
    characters: ConstPtr<unichar>,
    glyphs: MutPtr<CGGlyph>,
    count: i32,
) -> bool {
    if font.is_null() || characters.is_null() || glyphs.is_null() || count <= 0 {
        return false;
    }
    // Resolve the backing UIFont, then its rasterizable Font.
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    let font_obj = font_from_uifont(env, ui_font).unwrap_or_else(Font::sans_regular);
    let mut all_mapped = true;
    for i in 0..count as u32 {
        let c: unichar = env.mem.read(characters + i);
        let glyph_id = font_obj.glyph_id_for_char(c);
        let g = glyph_id.0;
        if g == 0 {
            all_mapped = false;
        }
        env.mem.write(glyphs + i, g);
    }
    all_mapped
}

/// `CGFontRef CTFontCopyGraphicsFont(CTFontRef font, CTFontDescriptorRef *attributes)`
///
/// Returns a `CGFont` for the given `CTFont`. We bridge through the backing
/// `UIFont` and create a `CGFont` from its name, so subsequent CGFont glyph
/// queries operate on the same typeface. The caller owns the returned font
/// (the "Copy" rule), so it is returned retained.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509342-ctfontcopygraphicsfont>
fn CTFontCopyGraphicsFont(
    env: &mut Environment,
    font: CTFontRef,
    attributes: MutPtr<CTFontDescriptorRef>,
) -> CGFontRef {
    if !attributes.is_null() {
        env.mem.write(attributes, nil);
    }
    if font.is_null() {
        return Ptr::null();
    }
    let ui_font = env.objc.borrow::<CTFontHostObject>(font).font;
    // Derive a font name from the UIFont and build a CGFont from it. This
    // matches what CoreText does (a CTFont and its CGFont share a typeface).
    let name: id = if ui_font != nil {
        msg![env; ui_font fontName]
    } else {
        nil
    };
    let cg_font = if name != nil {
        CGFontCreateWithFontName(env, name)
    } else {
        Ptr::null()
    };
    cg_font
}

/// `CTFramesetterRef CTFramesetterCreateWithAttributedString(CFAttributedStringRef string)`
///
/// Creates a framesetter that manages laying the attributed string into
/// frames. We retain the attributed string and wrap it in a host object so
/// frame creation and size suggestion can re-derive the text and font.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509611-ctframesettercreatewithattribute>
fn CTFramesetterCreateWithAttributedString(
    env: &mut Environment,
    string: CFAttributedStringRef,
) -> CTFramesetterRef {
    if string.is_null() {
        return nil;
    }
    retain(env, string);
    alloc_framesetter(env, string)
}

/// `CGSize CTFramesetterSuggestFrameSizeWithConstraints(CTFramesetterRef framesetter,
///     CFRange stringRange, CFDictionaryRef frameAttributes, CGSize constraints,
///     CFRange *fitRange)`
///
/// Determines the frame size needed for a string range. We measure the text
/// with the backing font, wrapping at the constraint width when one is given
/// (a width <= 0 or non-finite means "unconstrained"), and report the fit
/// range as the whole string. This produces correct sizing for the
/// single-/multi-line labels the apps in the corpus lay out.
///
/// Reference: <https://developer.apple.com/documentation/coretext/ctframesettersuggestframesizewithconstraints(_:_:_:_:_:)>
fn CTFramesetterSuggestFrameSizeWithConstraints(
    env: &mut Environment,
    framesetter: CTFramesetterRef,
    string_range: CFRange,
    _frame_attributes: id,
    constraints: CGSize,
    fit_range: MutPtr<CFRange>,
) -> CGSize {
    if framesetter.is_null() {
        if !fit_range.is_null() {
            env.mem.write(fit_range, CFRange { location: 0, length: 0 });
        }
        return CGSize { width: 0.0, height: 0.0 };
    }
    let attr_string = env.objc.borrow::<CTFramesetterHostObject>(framesetter).attr_string;
    let (text, font) = string_and_font_from_attr_string(env, attr_string);
    let size: CGFloat = msg![env; font pointSize];
    let text_str = to_rust_string(env, text).into_owned();
    let font_obj = font_from_uifont(env, font).unwrap_or_else(Font::sans_regular);

    let wrap = if constraints.width.is_finite() && constraints.width > 0.0 {
        Some((constraints.width, WrapMode::Word))
    } else {
        None
    };
    let (w, h) = font_obj.calculate_text_size(size, &text_str, wrap);

    let full_len = {
        let len: NSUInteger = msg![env; text length];
        len as i32
    };
    if !fit_range.is_null() {
        // Report the entire requested range as fitting. A length of 0 in the
        // requested range means "to the end of the string" per Apple's docs.
        let length = if string_range.length == 0 {
            (full_len - string_range.location).max(0)
        } else {
            string_range.length
        };
        env.mem.write(
            fit_range,
            CFRange {
                location: string_range.location,
                length,
            },
        );
    }
    CGSize { width: w, height: h }
}

/// `CTFrameRef CTFramesetterCreateFrame(CTFramesetterRef framesetter,
///     CFRange stringRange, CGPathRef path, CFDictionaryRef frameAttributes)`
///
/// Creates a frame laying the framesetter's attributed string into the
/// supplied path's bounding region. touchHLE represents the laid-out frame as
/// a single `CTLine` over the whole text; `CTFrameGetLines` then returns it.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509702-ctframesettercreateframe>
fn CTFramesetterCreateFrame(
    env: &mut Environment,
    framesetter: CTFramesetterRef,
    string_range: CFRange,
    _path: id,
    _frame_attributes: id,
) -> CTFrameRef {
    if framesetter.is_null() {
        return nil;
    }
    let attr_string = env.objc.borrow::<CTFramesetterHostObject>(framesetter).attr_string;
    let (text, font) = string_and_font_from_attr_string(env, attr_string);
    retain(env, text);
    retain(env, font);
    let line = alloc_line(env, text, font);
    // alloc_line took the retains above; the frame owns the line.
    let full_len = {
        let len: NSUInteger = msg![env; text length];
        len as i32
    };
    let range = CFRange {
        location: string_range.location,
        length: if string_range.length == 0 {
            (full_len - string_range.location).max(0)
        } else {
            string_range.length
        },
    };
    alloc_frame(env, line, range)
}

/// `CFArrayRef CTFrameGetLines(CTFrameRef frame)`
///
/// Returns the array of `CTLine`s that make up the frame. touchHLE lays the
/// frame out as a single line, so this returns a one-element array.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509601-ctframegetlines>
fn CTFrameGetLines(env: &mut Environment, frame: CTFrameRef) -> id {
    if frame.is_null() {
        return nil;
    }
    let line = env.objc.borrow::<CTFrameHostObject>(frame).line;
    let arr: id = msg_class![env; NSMutableArray arrayWithCapacity:1u32];
    if line != nil {
        () = msg![env; arr addObject:line];
    }
    let immutable: id = msg![env; arr copy];
    autorelease(env, immutable)
}

/// `void CTFrameGetLineOrigins(CTFrameRef frame, CFRange range, CGPoint origins[])`
///
/// Writes the origin of each line into `origins`. Our frame is a single line
/// whose origin sits at the top-left of the frame's coordinate space.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509559-ctframegetlineorigins>
fn CTFrameGetLineOrigins(
    env: &mut Environment,
    frame: CTFrameRef,
    range: CFRange,
    origins: MutPtr<CGPoint>,
) {
    if frame.is_null() || origins.is_null() {
        return;
    }
    let line = env.objc.borrow::<CTFrameHostObject>(frame).line;
    let line_count: i32 = if line != nil { 1 } else { 0 };
    let count = if range.length == 0 {
        line_count
    } else {
        range.length.min(line_count)
    };
    for i in 0..count as u32 {
        env.mem.write(origins + i, CGPoint { x: 0.0, y: 0.0 });
    }
}

/// `CFRange CTFrameGetStringRange(CTFrameRef frame)`
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509593-ctframegetstringrange>
fn CTFrameGetStringRange(env: &mut Environment, frame: CTFrameRef) -> CFRange {
    if frame.is_null() {
        return CFRange { location: 0, length: 0 };
    }
    env.objc.borrow::<CTFrameHostObject>(frame).range
}

/// `CFRange CTFrameGetVisibleStringRange(CTFrameRef frame)`
///
/// We lay the entire string into the (single-line) frame, so the visible
/// range equals the full string range.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509563-ctframegetvisiblestringrange>
fn CTFrameGetVisibleStringRange(env: &mut Environment, frame: CTFrameRef) -> CFRange {
    if frame.is_null() {
        return CFRange { location: 0, length: 0 };
    }
    env.objc.borrow::<CTFrameHostObject>(frame).range
}

/// `void CTFrameDraw(CTFrameRef frame, CGContextRef context)`
///
/// Draws the frame's lines into the context. We draw our single line at the
/// origin, matching `CTLineDraw`.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509601-ctframedraw>
fn CTFrameDraw(env: &mut Environment, frame: CTFrameRef, context: id) {
    if frame.is_null() || context.is_null() {
        return;
    }
    let line = env.objc.borrow::<CTFrameHostObject>(frame).line;
    if line != nil {
        CTLineDraw(env, line, context);
    }
}

/// `CTFontRef CTFontCreateWithGraphicsFont(CGFontRef graphicsFont,
///     CGFloat size, const CGAffineTransform *matrix,
///     CTFontDescriptorRef attributes)`
///
/// Creates a CTFont from a CGFont. We wrap a system UIFont at the requested
/// size in a real `_touchHLE_CTFont` host object so subsequent metric queries
/// (`CTFontGetAscent`/`Descent`/`Leading`/`Size`) and line drawing work.
///
/// Reference: <https://developer.apple.com/documentation/coretext/1509694-ctfontcreatewithgraphicsfont>
fn CTFontCreateWithGraphicsFont(
    env: &mut Environment,
    graphics_font: id, // CGFontRef
    size: CGFloat,
    _matrix: ConstVoidPtr, // const CGAffineTransform*
    _attributes: id,       // CTFontDescriptorRef
) -> CTFontRef {
    // A CGFont is toll-free convertible to a UIFont in our model only via the
    // font name; CGFont host objects don't carry a UIFont. Fall back to a
    // system font at the requested size so callers get a usable CTFont with
    // real metrics rather than NULL.
    let _ = graphics_font;
    let font_size = if size > 0.0 { size } else { 12.0 };
    let ui_font: id = msg_class![env; UIFont systemFontOfSize:font_size];
    retain(env, ui_font);
    alloc_font(env, ui_font)
}

/// `bool CTFontManagerRegisterGraphicsFont(CGFontRef font, CFErrorRef *error)`
///
/// Reference: <https://developer.apple.com/documentation/coretext/1499468-ctfontmanagerregistergraphicsfon>
fn CTFontManagerRegisterGraphicsFont(
    env: &mut Environment,
    font: ConstVoidPtr,             // CGFontRef
    error: MutPtr<crate::objc::id>, // CFErrorRef*
) -> bool {
    if !error.is_null() {
        env.mem.write(error, crate::objc::nil);
    }
    if font.is_null() {
        return false;
    }
    true
}

/// Opaque paragraph style reference.
pub type CTParagraphStyleRef = crate::objc::id;

/// `CTParagraphStyleRef CTParagraphStyleCreate(
///     const CTParagraphStyleSetting *settings, size_t settingCount)`
///
/// Reference: <https://developer.apple.com/documentation/coretext/1524171-ctparagraphstylecreate>
fn CTParagraphStyleCreate(
    env: &mut Environment,
    _settings: ConstVoidPtr,
    _setting_count: u32,
) -> CTParagraphStyleRef {
    msg_class![env; NSObject new]
}

/// `CTParagraphStyleRef CTParagraphStyleCreateCopy(CTParagraphStyleRef)`
///
/// Reference: <https://developer.apple.com/documentation/coretext/1525098-ctparagraphstylecreatecopy>
fn CTParagraphStyleCreateCopy(
    env: &mut Environment,
    paragraph_style: CTParagraphStyleRef,
) -> CTParagraphStyleRef {
    if paragraph_style.is_null() {
        return nil;
    }
    retain(env, paragraph_style);
    paragraph_style
}

/// `bool CTParagraphStyleGetValueForSpecifier(CTParagraphStyleRef, CTParagraphStyleSpecifier,
///     size_t valueBufferSize, void *valueBuffer)`
///
/// Reference: <https://developer.apple.com/documentation/coretext/1525353-ctparagraphstylegetvalueforspeci>
fn CTParagraphStyleGetValueForSpecifier(
    env: &mut Environment,
    _paragraph_style: CTParagraphStyleRef,
    _spec: u32,
    value_buffer_size: u32,
    value_buffer: MutPtr<u8>,
) -> bool {
    if !value_buffer.is_null() && value_buffer_size > 0 {
        let slice = env.mem.bytes_at_mut(value_buffer, value_buffer_size);
        slice.fill(0);
    }
    false
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CTFontCreateWithGraphicsFont(_, _, _, _)),
    export_c_func!(CTFontCreateWithFontDescriptor(_, _, _)),
    export_c_func!(CTFontDescriptorCreateWithAttributes(_)),
    export_c_func!(CTFontManagerRegisterGraphicsFont(_, _)),
    export_c_func!(CTFontGetAscent(_)),
    export_c_func!(CTFontGetDescent(_)),
    export_c_func!(CTFontGetLeading(_)),
    export_c_func!(CTFontGetSize(_)),
    export_c_func!(CTParagraphStyleCreate(_, _)),
    export_c_func!(CTParagraphStyleCreateCopy(_)),
    export_c_func!(CTParagraphStyleGetValueForSpecifier(_, _, _, _)),
    export_c_func!(CFAttributedStringCreate(_, _, _)),
    export_c_func!(CFAttributedStringCreateCopy(_, _)),
    export_c_func!(CFAttributedStringGetString(_)),
    export_c_func!(CTTypesetterCreateWithAttributedString(_)),
    export_c_func!(CTTypesetterCreateLine(_, _)),
    export_c_func!(CTLineCreateWithAttributedString(_)),
    export_c_func!(CTLineGetTypographicBounds(_, _, _, _)),
    export_c_func!(CTLineGetImageBounds(_, _)),
    export_c_func!(CTLineDraw(_, _)),
    export_c_func!(CTLineGetGlyphCount(_)),
    export_c_func!(CTLineGetGlyphRuns(_)),
    export_c_func!(CTFontGetGlyphsForCharacters(_, _, _, _)),
    export_c_func!(CTFontCopyGraphicsFont(_, _)),
    export_c_func!(CTFramesetterCreateWithAttributedString(_)),
    export_c_func!(CTFramesetterSuggestFrameSizeWithConstraints(_, _, _, _, _)),
    export_c_func!(CTFramesetterCreateFrame(_, _, _, _)),
    export_c_func!(CTFrameGetLines(_)),
    export_c_func!(CTFrameGetLineOrigins(_, _, _)),
    export_c_func!(CTFrameGetStringRange(_)),
    export_c_func!(CTFrameGetVisibleStringRange(_)),
    export_c_func!(CTFrameDraw(_, _)),
];

pub const CONSTANTS: ConstantExports = &[
    // CTFontDescriptor.h
    (
        "_kCTFontNameAttribute",
        HostConstant::NSString("NSFontNameAttribute"),
    ),
    (
        "_kCTFontFamilyNameAttribute",
        HostConstant::NSString("NSFontFamilyAttribute"),
    ),
    (
        "_kCTFontStyleNameAttribute",
        HostConstant::NSString("NSFontFaceAttribute"),
    ),
    (
        "_kCTFontTraitsAttribute",
        HostConstant::NSString("NSCTFontTraitsAttribute"),
    ),
    (
        "_kCTFontURLAttribute",
        HostConstant::NSString("NSCTFontFileURLAttribute"),
    ),
    (
        "_kCTFontDisplayNameAttribute",
        HostConstant::NSString("NSFontVisibleNameAttribute"),
    ),
    (
        "_kCTFontSizeAttribute",
        HostConstant::NSString("NSFontSizeAttribute"),
    ),
    (
        "_kCTFontMatrixAttribute",
        HostConstant::NSString("NSCTFontMatrixAttribute"),
    ),
    (
        "_kCTFontCascadeListAttribute",
        HostConstant::NSString("NSCTFontCascadeListAttribute"),
    ),
    (
        "_kCTFontCharacterSetAttribute",
        HostConstant::NSString("NSCTFontCharacterSetAttribute"),
    ),
    (
        "_kCTFontLanguagesAttribute",
        HostConstant::NSString("NSCTFontLanguagesAttribute"),
    ),
    (
        "_kCTFontBaselineAdjustAttribute",
        HostConstant::NSString("NSCTFontBaselineAdjustAttribute"),
    ),
    (
        "_kCTFontMacintoshEncodingsAttribute",
        HostConstant::NSString("NSCTFontMacintoshEncodingsAttribute"),
    ),
    (
        "_kCTFontFeaturesAttribute",
        HostConstant::NSString("NSCTFontFeaturesAttribute"),
    ),
    (
        "_kCTFontFeatureSettingsAttribute",
        HostConstant::NSString("NSCTFontFeatureSettingsAttribute"),
    ),
    (
        "_kCTFontFixedAdvanceAttribute",
        HostConstant::NSString("NSCTFontFixedAdvanceAttribute"),
    ),
    (
        "_kCTFontOrientationAttribute",
        HostConstant::NSString("NSCTFontOrientationAttribute"),
    ),
    (
        "_kCTFontFormatAttribute",
        HostConstant::NSString("NSCTFontFormatAttribute"),
    ),
    (
        "_kCTFontRegistrationScopeAttribute",
        HostConstant::NSString("NSCTFontRegistrationScopeAttribute"),
    ),
    (
        "_kCTFontPriorityAttribute",
        HostConstant::NSString("NSCTFontPriorityAttribute"),
    ),
    (
        "_kCTFontEnabledAttribute",
        HostConstant::NSString("NSCTFontEnabledAttribute"),
    ),
    (
        "_kCTFontDownloadableAttribute",
        HostConstant::NSString("NSCTFontDownloadableAttribute"),
    ),
    (
        "_kCTFontDownloadedAttribute",
        HostConstant::NSString("NSCTFontDownloadedAttribute"),
    ),
    // CTFontTraits.h
    (
        "_kCTFontSymbolicTrait",
        HostConstant::NSString("NSCTFontSymbolicTrait"),
    ),
    (
        "_kCTFontWeightTrait",
        HostConstant::NSString("NSCTFontWeightTrait"),
    ),
    (
        "_kCTFontWidthTrait",
        HostConstant::NSString("NSCTFontWidthTrait"),
    ),
    (
        "_kCTFontSlantTrait",
        HostConstant::NSString("NSCTFontSlantTrait"),
    ),
    // CTStringAttributes.h — attribute keys for `CFAttributedStringRef`
    // (and, toll-free bridged, `NSAttributedString`) used by
    // `CTFramesetterCreateWithAttributedString` and friends.
    // Canonical string values come from Apple's public
    // `CTStringAttributes.h` header; many deliberately share their
    // value with the corresponding `NSAttributedString` attribute name
    // so the same dictionary can be used by both CoreText and
    // UIKit/AppKit.
    (
        "_kCTFontAttributeName",
        // Same value as `NSFontAttributeName`.
        HostConstant::NSString("NSFont"),
    ),
    (
        "_kCTForegroundColorAttributeName",
        HostConstant::NSString("CTForegroundColor"),
    ),
    (
        "_kCTForegroundColorFromContextAttributeName",
        HostConstant::NSString("CTForegroundColorFromContext"),
    ),
    (
        "_kCTBackgroundColorAttributeName",
        HostConstant::NSString("kCTBackgroundColorAttributeName"),
    ),
    (
        "_kCTKernAttributeName",
        // Same value as `NSKernAttributeName`.
        HostConstant::NSString("NSKern"),
    ),
    (
        "_kCTLigatureAttributeName",
        // Same value as `NSLigatureAttributeName`.
        HostConstant::NSString("NSLigature"),
    ),
    (
        "_kCTParagraphStyleAttributeName",
        // Same value as `NSParagraphStyleAttributeName`.
        HostConstant::NSString("NSParagraphStyle"),
    ),
    (
        "_kCTStrokeWidthAttributeName",
        // Same value as `NSStrokeWidthAttributeName`.
        HostConstant::NSString("NSStrokeWidth"),
    ),
    (
        "_kCTStrokeColorAttributeName",
        // Same value as `NSStrokeColorAttributeName`.
        HostConstant::NSString("NSStrokeColor"),
    ),
    (
        "_kCTUnderlineStyleAttributeName",
        HostConstant::NSString("CTUnderlineStyle"),
    ),
    (
        "_kCTUnderlineColorAttributeName",
        HostConstant::NSString("CTUnderlineColor"),
    ),
    (
        "_kCTSuperscriptAttributeName",
        // Same value as `NSSuperscriptAttributeName`.
        HostConstant::NSString("NSSuperScript"),
    ),
    (
        "_kCTVerticalFormsAttributeName",
        HostConstant::NSString("CTVerticalForms"),
    ),
    (
        "_kCTGlyphInfoAttributeName",
        HostConstant::NSString("CTGlyphInfo"),
    ),
    (
        "_kCTCharacterShapeAttributeName",
        // Same value as `NSCharacterShapeAttributeName`.
        HostConstant::NSString("NSCharacterShape"),
    ),
    (
        "_kCTLanguageAttributeName",
        HostConstant::NSString("CTLanguage"),
    ),
    (
        "_kCTRunDelegateAttributeName",
        HostConstant::NSString("CTRunDelegate"),
    ),
    (
        "_kCTBaselineClassAttributeName",
        HostConstant::NSString("CTBaselineClass"),
    ),
    (
        "_kCTBaselineInfoAttributeName",
        HostConstant::NSString("CTBaselineInfo"),
    ),
    (
        "_kCTBaselineReferenceInfoAttributeName",
        HostConstant::NSString("CTBaselineReferenceInfo"),
    ),
    (
        "_kCTBaselineOffsetAttributeName",
        // Same value as `NSBaselineOffsetAttributeName`.
        HostConstant::NSString("NSBaselineOffset"),
    ),
    (
        "_kCTWritingDirectionAttributeName",
        // Same value as `NSWritingDirectionAttributeName`.
        HostConstant::NSString("NSWritingDirection"),
    ),
    (
        "_kCTTrackingAttributeName",
        HostConstant::NSString("CTTracking"),
    ),
];

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/CoreText.framework/CoreText",
    aliases: &[],
    class_exports: &[CLASSES],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};
