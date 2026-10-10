/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSParagraphStyle` and `NSMutableParagraphStyle`.
//!
//! On iPhone OS these live in UIKit (`UIKit/NSParagraphStyle.h`, iOS 6+).
//! They are plain value objects describing paragraph layout, used as the
//! value of `NSParagraphStyleAttributeName` in attributed strings and in the
//! attribute dictionaries passed to the `NSString` drawing methods.
//!
//! References:
//! - <https://developer.apple.com/documentation/uikit/nsparagraphstyle>
//! - <https://developer.apple.com/documentation/uikit/nsmutableparagraphstyle>

use crate::frameworks::core_graphics::CGFloat;
use crate::frameworks::foundation::ns_string::from_rust_string;
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;

/// `NSTextAlignmentNatural`
pub const NS_TEXT_ALIGNMENT_NATURAL: NSInteger = 4;
/// `NSLineBreakByWordWrapping`
pub const NS_LINE_BREAK_BY_WORD_WRAPPING: NSInteger = 0;
/// `NSWritingDirectionNatural`
pub const NS_WRITING_DIRECTION_NATURAL: NSInteger = -1;
/// `NSWritingDirectionLeftToRight`
pub const NS_WRITING_DIRECTION_LEFT_TO_RIGHT: NSInteger = 0;
/// `NSWritingDirectionRightToLeft`
pub const NS_WRITING_DIRECTION_RIGHT_TO_LEFT: NSInteger = 1;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ParagraphStyleValues {
    pub alignment: NSInteger,
    pub line_break_mode: NSInteger,
    pub base_writing_direction: NSInteger,
    pub line_spacing: CGFloat,
    pub paragraph_spacing: CGFloat,
    pub paragraph_spacing_before: CGFloat,
    pub head_indent: CGFloat,
    pub tail_indent: CGFloat,
    pub first_line_head_indent: CGFloat,
    pub minimum_line_height: CGFloat,
    pub maximum_line_height: CGFloat,
    pub line_height_multiple: CGFloat,
    pub hyphenation_factor: f32,
    pub default_tab_interval: CGFloat,
    pub allows_default_tightening_for_truncation: bool,
}

impl Default for ParagraphStyleValues {
    /// The values of `+[NSParagraphStyle defaultParagraphStyle]`.
    fn default() -> Self {
        ParagraphStyleValues {
            alignment: NS_TEXT_ALIGNMENT_NATURAL,
            line_break_mode: NS_LINE_BREAK_BY_WORD_WRAPPING,
            base_writing_direction: NS_WRITING_DIRECTION_NATURAL,
            line_spacing: 0.0,
            paragraph_spacing: 0.0,
            paragraph_spacing_before: 0.0,
            head_indent: 0.0,
            tail_indent: 0.0,
            first_line_head_indent: 0.0,
            minimum_line_height: 0.0,
            maximum_line_height: 0.0,
            line_height_multiple: 0.0,
            hyphenation_factor: 0.0,
            default_tab_interval: 0.0,
            allows_default_tightening_for_truncation: false,
        }
    }
}

#[derive(Default)]
struct NSParagraphStyleHostObject {
    values: ParagraphStyleValues,
    /// Retained `NSArray` of `NSTextTab`s, or `nil` for "none set" (reported
    /// as an empty array).
    tab_stops: id,
}
impl HostObject for NSParagraphStyleHostObject {}

fn values(env: &Environment, this: id) -> ParagraphStyleValues {
    env.objc.borrow::<NSParagraphStyleHostObject>(this).values
}

fn values_mut(env: &mut Environment, this: id) -> &mut ParagraphStyleValues {
    &mut env.objc.borrow_mut::<NSParagraphStyleHostObject>(this).values
}

/// Returns the paragraph-style values of `obj` if it is an
/// `NSParagraphStyle` (or subclass) instance. Useful for text drawing code
/// that receives a style through an attribute dictionary.
pub fn paragraph_style_values(env: &Environment, obj: id) -> Option<ParagraphStyleValues> {
    if obj == nil {
        return None;
    }
    env.objc
        .get_host_object(obj)
        .and_then(|host| host.as_any().downcast_ref::<NSParagraphStyleHostObject>())
        .map(|host| host.values)
}

fn set_tab_stops(env: &mut Environment, this: id, tab_stops: id) {
    let new: id = if tab_stops == nil {
        nil
    } else {
        msg![env; tab_stops copy]
    };
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<NSParagraphStyleHostObject>(this).tab_stops,
        new,
    );
    release(env, old);
}

/// Copy all state of `src` into `dst` (both paragraph styles).
fn copy_style_into(env: &mut Environment, src: id, dst: id) {
    let (values, tab_stops) = {
        let host = env.objc.borrow::<NSParagraphStyleHostObject>(src);
        (host.values, host.tab_stops)
    };
    *values_mut(env, dst) = values;
    set_tab_stops(env, dst, tab_stops);
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSParagraphStyle: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSParagraphStyleHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)defaultParagraphStyle {
    let style: id = msg_class![env; NSParagraphStyle alloc];
    let style: id = msg![env; style init];
    autorelease(env, style)
}

+ (NSInteger)defaultWritingDirectionForLanguage:(id)language_name {
    // Right-to-left scripts per ISO 639 code; everything else is LTR.
    if language_name == nil {
        return NS_WRITING_DIRECTION_LEFT_TO_RIGHT;
    }
    let name = crate::frameworks::foundation::ns_string::to_rust_string(env, language_name);
    let primary = name
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match primary.as_str() {
        "ar" | "arc" | "dv" | "fa" | "ha" | "he" | "iw" | "khw" | "ks" | "ku" | "ps" | "sd"
        | "syr" | "ur" | "yi" => NS_WRITING_DIRECTION_RIGHT_TO_LEFT,
        _ => NS_WRITING_DIRECTION_LEFT_TO_RIGHT,
    }
}

- (())dealloc {
    let tab_stops = std::mem::replace(
        &mut env.objc.borrow_mut::<NSParagraphStyleHostObject>(this).tab_stops,
        nil,
    );
    release(env, tab_stops);
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Getters

- (NSInteger)alignment {
    values(env, this).alignment
}
- (NSInteger)lineBreakMode {
    values(env, this).line_break_mode
}
- (NSInteger)baseWritingDirection {
    values(env, this).base_writing_direction
}
- (CGFloat)lineSpacing {
    values(env, this).line_spacing
}
- (CGFloat)paragraphSpacing {
    values(env, this).paragraph_spacing
}
- (CGFloat)paragraphSpacingBefore {
    values(env, this).paragraph_spacing_before
}
- (CGFloat)headIndent {
    values(env, this).head_indent
}
- (CGFloat)tailIndent {
    values(env, this).tail_indent
}
- (CGFloat)firstLineHeadIndent {
    values(env, this).first_line_head_indent
}
- (CGFloat)minimumLineHeight {
    values(env, this).minimum_line_height
}
- (CGFloat)maximumLineHeight {
    values(env, this).maximum_line_height
}
- (CGFloat)lineHeightMultiple {
    values(env, this).line_height_multiple
}
- (f32)hyphenationFactor {
    values(env, this).hyphenation_factor
}
- (CGFloat)defaultTabInterval {
    values(env, this).default_tab_interval
}
- (bool)allowsDefaultTighteningForTruncation {
    values(env, this).allows_default_tightening_for_truncation
}
- (id)tabStops {
    let tab_stops = env.objc.borrow::<NSParagraphStyleHostObject>(this).tab_stops;
    if tab_stops == nil {
        msg_class![env; NSArray array]
    } else {
        let tab_stops = retain(env, tab_stops);
        autorelease(env, tab_stops)
    }
}

// MARK: - NSCopying / NSMutableCopying

- (id)copyWithZone:(NSZonePtr)_zone {
    let copy: id = msg_class![env; NSParagraphStyle alloc];
    copy_style_into(env, this, copy);
    copy
}
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let copy: id = msg_class![env; NSMutableParagraphStyle alloc];
    copy_style_into(env, this, copy);
    copy
}

// MARK: - NSObject

- (bool)isEqual:(id)other {
    if this == other {
        return true;
    }
    let Some(other_values) = paragraph_style_values(env, other) else {
        return false;
    };
    if values(env, this) != other_values {
        return false;
    }
    let a: id = msg![env; this tabStops];
    let b: id = msg![env; other tabStops];
    msg![env; a isEqualToArray:b]
}

- (NSUInteger)hash {
    let v = values(env, this);
    (v.alignment as NSUInteger)
        ^ ((v.line_break_mode as NSUInteger) << 4)
        ^ (v.line_spacing.to_bits() as NSUInteger).rotate_left(8)
        ^ (v.head_indent.to_bits() as NSUInteger).rotate_left(16)
        ^ (v.first_line_head_indent.to_bits() as NSUInteger).rotate_left(24)
}

- (id)description {
    let v = values(env, this);
    let desc = format!(
        "Alignment {}, LineSpacing {}, ParagraphSpacing {}, ParagraphSpacingBefore {}, \
         HeadIndent {}, TailIndent {}, FirstLineHeadIndent {}, LineHeight {}/{}, \
         LineHeightMultiple {}, LineBreakMode {}, BaseWritingDirection {}, \
         HyphenationFactor {}, DefaultTabInterval {}",
        v.alignment,
        v.line_spacing,
        v.paragraph_spacing,
        v.paragraph_spacing_before,
        v.head_indent,
        v.tail_indent,
        v.first_line_head_indent,
        v.minimum_line_height,
        v.maximum_line_height,
        v.line_height_multiple,
        v.line_break_mode,
        v.base_writing_direction,
        v.hyphenation_factor,
        v.default_tab_interval,
    );
    let desc = from_rust_string(env, desc);
    autorelease(env, desc)
}

@end

@implementation NSMutableParagraphStyle: NSParagraphStyle

- (())setAlignment:(NSInteger)alignment {
    values_mut(env, this).alignment = alignment;
}
- (())setLineBreakMode:(NSInteger)mode {
    values_mut(env, this).line_break_mode = mode;
}
- (())setBaseWritingDirection:(NSInteger)direction {
    values_mut(env, this).base_writing_direction = direction;
}
- (())setLineSpacing:(CGFloat)value {
    // Negative values are not allowed for the spacing/indent/height
    // properties marked "This value is always nonnegative" in the header.
    values_mut(env, this).line_spacing = value.max(0.0);
}
- (())setParagraphSpacing:(CGFloat)value {
    values_mut(env, this).paragraph_spacing = value.max(0.0);
}
- (())setParagraphSpacingBefore:(CGFloat)value {
    values_mut(env, this).paragraph_spacing_before = value.max(0.0);
}
- (())setHeadIndent:(CGFloat)value {
    values_mut(env, this).head_indent = value.max(0.0);
}
- (())setTailIndent:(CGFloat)value {
    // Negative tail indents are meaningful (distance from the trailing
    // margin), so this one is stored as-is.
    values_mut(env, this).tail_indent = value;
}
- (())setFirstLineHeadIndent:(CGFloat)value {
    values_mut(env, this).first_line_head_indent = value.max(0.0);
}
- (())setMinimumLineHeight:(CGFloat)value {
    values_mut(env, this).minimum_line_height = value.max(0.0);
}
- (())setMaximumLineHeight:(CGFloat)value {
    values_mut(env, this).maximum_line_height = value.max(0.0);
}
- (())setLineHeightMultiple:(CGFloat)value {
    values_mut(env, this).line_height_multiple = value.max(0.0);
}
- (())setHyphenationFactor:(f32)value {
    values_mut(env, this).hyphenation_factor = value.clamp(0.0, 1.0);
}
- (())setDefaultTabInterval:(CGFloat)value {
    values_mut(env, this).default_tab_interval = value.max(0.0);
}
- (())setAllowsDefaultTighteningForTruncation:(bool)value {
    values_mut(env, this).allows_default_tightening_for_truncation = value;
}
- (())setTabStops:(id)tab_stops {
    set_tab_stops(env, this, tab_stops);
}
- (())addTabStop:(id)tab {
    if tab == nil {
        return;
    }
    let current: id = msg![env; this tabStops];
    let updated: id = msg![env; current arrayByAddingObject:tab];
    set_tab_stops(env, this, updated);
}
- (())removeTabStop:(id)tab {
    if tab == nil {
        return;
    }
    let current: id = msg![env; this tabStops];
    let updated: id = msg![env; current mutableCopy];
    () = msg![env; updated removeObject:tab];
    set_tab_stops(env, this, updated);
    release(env, updated);
}
- (())setParagraphStyle:(id)other {
    if other == nil || other == this {
        return;
    }
    if paragraph_style_values(env, other).is_none() {
        log!("Warning: -[NSMutableParagraphStyle setParagraphStyle:] got non-style {:?}", other);
        return;
    }
    copy_style_into(env, other, this);
}

@end

};
