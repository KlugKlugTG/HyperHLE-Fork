/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UILabel`.

use crate::frameworks::core_graphics::cg_context::CGContextSetRGBFillColor;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::frameworks::uikit::ui_color;
use crate::frameworks::uikit::ui_font::{
    UILineBreakMode, UILineBreakModeTailTruncation, UITextAlignment, UITextAlignmentCenter,
    UITextAlignmentLeft, UITextAlignmentRight,
};
use crate::frameworks::uikit::ui_graphics::UIGraphicsGetCurrentContext;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, release,
    retain, ClassExports, NSZonePtr,
};

type UIBaselineAdjustment = NSInteger;

pub struct UILabelHostObject {
    superclass: super::UIViewHostObject,
    text: id,
    font: id,
    text_color: id,
    highlighted_text_color: id,
    text_alignment: UITextAlignment,
    line_break_mode: UILineBreakMode,
    number_of_lines: NSInteger,
    enabled: bool,
    adjusts_font_size_to_fit_width: bool,
    minimum_font_size: CGFloat,
    baseline_adjustment: UIBaselineAdjustment,
    shadow_color: id,
    shadow_offset: CGSize,
}
impl_HostObject_with_superclass!(UILabelHostObject);

impl Default for UILabelHostObject {
    fn default() -> Self {
        UILabelHostObject {
            superclass: Default::default(),
            text: nil,
            font: nil,
            text_color: nil,
            highlighted_text_color: nil,
            text_alignment: UITextAlignmentLeft,
            line_break_mode: UILineBreakModeTailTruncation,
            number_of_lines: 1,
            enabled: true,
            adjusts_font_size_to_fit_width: false,
            minimum_font_size: 0.0,
            baseline_adjustment: 0,
            // Apple's documented defaults for UILabel:
            // - shadowColor: nil (no shadow drawn)
            // - shadowOffset: { width: 0, height: -1 } (one point above the
            //   baseline, mirroring UIKit's behaviour on iOS 2.x–6.x).
            shadow_color: nil,
            shadow_offset: CGSize {
                width: 0.0,
                height: -1.0,
            },
        }
    }
}

fn fitting_font_for_width(
    env: &mut crate::Environment,
    text: id,
    font: id,
    width: CGFloat,
    adjusts: bool,
    minimum_font_size: CGFloat,
) -> id {
    if !adjusts || width <= 0.0 {
        return font;
    }
    let text_size: CGSize = msg![env; text sizeWithFont:font];
    if text_size.width <= width || text_size.width <= 0.0 {
        return font;
    }
    let point_size: CGFloat = msg![env; font pointSize];
    if point_size <= 0.0 {
        return font;
    }
    let min_size = if minimum_font_size > 0.0 {
        minimum_font_size.min(point_size)
    } else {
        point_size
    };
    let scaled_size = (point_size * (width / text_size.width)).clamp(min_size, point_size);
    if (scaled_size - point_size).abs() < 0.01 {
        font
    } else {
        msg![env; font fontWithSize:scaled_size]
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UILabel: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UILabelHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];

    let key_text = get_static_str(env, "UIText");
    if msg![env; coder containsValueForKey:key_text] {
        let text: id = msg![env; coder decodeObjectForKey:key_text];
        () = msg![env; this setText:text];
    }

    let key_font = get_static_str(env, "UIFont");
    if msg![env; coder containsValueForKey:key_font] {
        let font: id = msg![env; coder decodeObjectForKey:key_font];
        () = msg![env; this setFont:font];
    } else {
        () = msg![env; this setFont:nil];
    }

    let key_color = get_static_str(env, "UITextColor");
    if msg![env; coder containsValueForKey:key_color] {
        let text_color: id = msg![env; coder decodeObjectForKey:key_color];
        () = msg![env; this setTextColor:text_color];
    } else {
        () = msg![env; this setTextColor:nil];
    }

    let key_align = get_static_str(env, "UITextAlignment");
    if msg![env; coder containsValueForKey:key_align] {
        let align: UITextAlignment = msg![env; coder decodeIntegerForKey:key_align];
        () = msg![env; this setTextAlignment:align];
    }

    let key_lines = get_static_str(env, "UINumberOfLines");
    if msg![env; coder containsValueForKey:key_lines] {
        let lines: NSInteger = msg![env; coder decodeIntegerForKey:key_lines];
        () = msg![env; this setNumberOfLines:lines];
    }

    let key_break = get_static_str(env, "UILineBreakMode");
    if msg![env; coder containsValueForKey:key_break] {
        let break_mode: UILineBreakMode = msg![env; coder decodeIntegerForKey:key_break];
        () = msg![env; this setLineBreakMode:break_mode];
    }

    let key_bg = get_static_str(env, "UIBackgroundColor");
    let bg_color = if msg![env; coder containsValueForKey:key_bg] {
        msg![env; coder decodeObjectForKey:key_bg]
    } else {
        nil
    };

    if bg_color == nil {
        let clear_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; this setBackgroundColor:clear_color];
    } else {
        () = msg![env; this setBackgroundColor:bg_color];
    }

    () = msg_super![env; this setOpaque:false];
    this
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    () = msg![env; this setFont:nil];
    () = msg![env; this setTextColor:nil];
    () = msg![env; this setBackgroundColor:nil];
    () = msg_super![env; this setOpaque:false];
    this
}

- (())dealloc {
    let &UILabelHostObject { text, font, text_color, highlighted_text_color, shadow_color, .. } = env.objc.borrow(this);
    release(env, text);
    release(env, font);
    release(env, text_color);
    release(env, highlighted_text_color);
    release(env, shadow_color);
    msg_super![env; this dealloc]
}

// MARK: - Shadow (UILabel + UIStringDrawing semantics)
//
// Apple documentation:
//   - https://developer.apple.com/documentation/uikit/uilabel/1620539-shadowcolor
//   - https://developer.apple.com/documentation/uikit/uilabel/1620546-shadowoffset
//
// `shadowColor` is a retained `UIColor` (nil = no shadow). `shadowOffset` is
// a `CGSize` whose `height` is in *UIKit* coordinates: a negative value pulls
// the shadow above the baseline (the iPhone OS default is { 0, -1 }).

- (id)shadowColor { env.objc.borrow::<UILabelHostObject>(this).shadow_color }
- (())setShadowColor:(id)new_color {
    let old_color = std::mem::replace(
        &mut env.objc.borrow_mut::<UILabelHostObject>(this).shadow_color,
        new_color,
    );
    retain(env, new_color);
    release(env, old_color);
    () = msg![env; this setNeedsDisplay];
}

- (CGSize)shadowOffset { env.objc.borrow::<UILabelHostObject>(this).shadow_offset }
- (())setShadowOffset:(CGSize)new_offset {
    env.objc.borrow_mut::<UILabelHostObject>(this).shadow_offset = new_offset;
    () = msg![env; this setNeedsDisplay];
}

- (id)text { env.objc.borrow::<UILabelHostObject>(this).text }
- (())setText:(id)new_text {
    let new_text: id = if new_text != nil { msg![env; new_text copy] } else { nil };
    let old_text = std::mem::replace(&mut env.objc.borrow_mut::<UILabelHostObject>(this).text, new_text);
    release(env, old_text);
    () = msg![env; this setNeedsDisplay];
}

- (id)font { env.objc.borrow::<UILabelHostObject>(this).font }
- (())setFont:(id)new_font {
    let new_font: id = if new_font == nil {
        let size: CGFloat = 17.0;
        msg_class![env; UIFont systemFontOfSize:size]
    } else { new_font };

    let old_font = std::mem::replace(&mut env.objc.borrow_mut::<UILabelHostObject>(this).font, new_font);
    retain(env, new_font);
    release(env, old_font);
    () = msg![env; this setNeedsDisplay];
}

- (bool)adjustsFontSizeToFitWidth {
    env.objc.borrow::<UILabelHostObject>(this).adjusts_font_size_to_fit_width
}
- (())setAdjustsFontSizeToFitWidth:(bool)adjusts {
    env.objc.borrow_mut::<UILabelHostObject>(this).adjusts_font_size_to_fit_width = adjusts;
    () = msg![env; this setNeedsDisplay];
}

- (CGFloat)minimumFontSize {
    env.objc.borrow::<UILabelHostObject>(this).minimum_font_size
}
- (())setMinimumFontSize:(CGFloat)font_size {
    env.objc.borrow_mut::<UILabelHostObject>(this).minimum_font_size = font_size.max(0.0);
    () = msg![env; this setNeedsDisplay];
}

- (bool)isEnabled { env.objc.borrow::<UILabelHostObject>(this).enabled }
- (())setEnabled:(bool)enabled {
    env.objc.borrow_mut::<UILabelHostObject>(this).enabled = enabled;
    () = msg![env; this setNeedsDisplay];
}

- (id)textColor { env.objc.borrow::<UILabelHostObject>(this).text_color }
- (())setTextColor:(id)new_text_color {
    let new_text_color: id = if new_text_color == nil {
        msg_class![env; UIColor blackColor]
    } else { new_text_color };

    let old_text_color = std::mem::replace(&mut env.objc.borrow_mut::<UILabelHostObject>(this).text_color, new_text_color);
    retain(env, new_text_color);
    release(env, old_text_color);
    () = msg![env; this setNeedsDisplay];
}

- (id)highlightedTextColor { env.objc.borrow::<UILabelHostObject>(this).highlighted_text_color }
- (())setHighlightedTextColor:(id)new_color {
    let old_color = std::mem::replace(&mut env.objc.borrow_mut::<UILabelHostObject>(this).highlighted_text_color, new_color);
    retain(env, new_color);
    release(env, old_color);
    () = msg![env; this setNeedsDisplay];
}

- (())setBackgroundColor:(id)color {
    let color: id = if color == nil { msg_class![env; UIColor whiteColor] } else { color };
    msg_super![env; this setBackgroundColor:color]
}

- (())setOpaque:(bool)_opaque { }

- (UITextAlignment)textAlignment { env.objc.borrow::<UILabelHostObject>(this).text_alignment }
- (())setTextAlignment:(UITextAlignment)text_alignment {
    env.objc.borrow_mut::<UILabelHostObject>(this).text_alignment = text_alignment;
    () = msg![env; this setNeedsDisplay];
}

- (UILineBreakMode)lineBreakMode { env.objc.borrow::<UILabelHostObject>(this).line_break_mode }
- (())setLineBreakMode:(UILineBreakMode)line_break_mode {
    env.objc.borrow_mut::<UILabelHostObject>(this).line_break_mode = line_break_mode;
    () = msg![env; this setNeedsDisplay];
}

- (NSInteger)numberOfLines { env.objc.borrow::<UILabelHostObject>(this).number_of_lines }
- (())setNumberOfLines:(NSInteger)number {
    env.objc.borrow_mut::<UILabelHostObject>(this).number_of_lines = number;
    () = msg![env; this setNeedsDisplay];
}

- (UIBaselineAdjustment)baselineAdjustment {
    env.objc.borrow::<UILabelHostObject>(this).baseline_adjustment
}
- (())setBaselineAdjustment:(UIBaselineAdjustment)value {
    env.objc.borrow_mut::<UILabelHostObject>(this).baseline_adjustment = value;
    () = msg![env; this setNeedsDisplay];
}

- (())sizeToFit {
    let size: CGSize = msg![env; this sizeThatFits:(CGSize { width: CGFloat::MAX, height: CGFloat::MAX })];
    let origin: CGPoint = {
        let frame: CGRect = msg![env; this frame];
        frame.origin
    };
    let new_frame = CGRect { origin, size };
    () = msg![env; this setFrame:new_frame];
}

- (CGSize)sizeThatFits:(CGSize)size {
    let &UILabelHostObject {
        text, font, line_break_mode, number_of_lines,
        adjusts_font_size_to_fit_width, minimum_font_size, ..
    } = env.objc.borrow(this);

    if text == nil || font == nil {
        return CGSize { width: 0.0, height: 0.0 };
    }

    let len: NSUInteger = msg![env; text length];
    if len == 0 {
        // Even with empty text, UILabel reports one line height
        let line_height: CGFloat = msg![env; font lineHeight];
        return CGSize { width: 0.0, height: line_height };
    }

    let single_line = number_of_lines == 1;

    if single_line {
        let font = fitting_font_for_width(
            env,
            text,
            font,
            size.width,
            adjusts_font_size_to_fit_width,
            minimum_font_size,
        );
        let text_size: CGSize = msg![env; text sizeWithFont:font];
        text_size
    } else {
        // Constrain to the proposed width (or infinite if unconstrained),
        // with effectively unlimited height for multi-line labels.
        let max_height = if number_of_lines == 0 {
            // 0 means unlimited lines
            CGFloat::MAX
        } else {
            // number_of_lines > 1: allow that many lines worth of height
            let line_height: CGFloat = msg![env; font lineHeight];
            line_height * (number_of_lines as CGFloat) + 1.0
        };
        let constraint = CGSize { width: size.width, height: max_height };
        let text_size: CGSize = msg![env; text sizeWithFont:font
                  constrainedToSize:constraint
                      lineBreakMode:line_break_mode];
        text_size
    }
}

- (())drawRect:(CGRect)_rect {
    let bounds: CGRect = msg![env; this bounds];
    let context = UIGraphicsGetCurrentContext(env);

    let &mut UILabelHostObject {
        text, font, text_color, text_alignment, line_break_mode, number_of_lines,
        adjusts_font_size_to_fit_width, minimum_font_size, baseline_adjustment,
        shadow_color, shadow_offset, ..
    } = env.objc.borrow_mut(this);

    if text == nil || font == nil || text_color == nil { return; }

    let len: NSUInteger = msg![env; text length];
    if len == 0 { return; }

    let single_line = number_of_lines == 1;

    let font = if single_line {
        fitting_font_for_width(
            env,
            text,
            font,
            bounds.size.width,
            adjusts_font_size_to_fit_width,
            minimum_font_size,
        )
    } else {
        font
    };

    let calculated_size: CGSize = if single_line {
        msg![env; text sizeWithFont:font]
    } else {
        msg![env; text sizeWithFont:font
              constrainedToSize:(bounds.size)
                  lineBreakMode:line_break_mode]
    };

    let y_offset = match baseline_adjustment {
        0 => 0.0,
        1 => (bounds.size.height - calculated_size.height) / 2.0,
        2 => bounds.size.height - calculated_size.height,
        _ => 0.0,
    };
    let base_rect = CGRect {
        origin: CGPoint { x: bounds.origin.x, y: bounds.origin.y + y_offset },
        size: CGSize { width: bounds.size.width, height: calculated_size.height },
    };

    let x_offset = match text_alignment {
        UITextAlignmentLeft => 0.0,
        UITextAlignmentCenter => 0.5,
        UITextAlignmentRight => 1.0,
        _ => 0.0,
    };
    let base_point = CGPoint {
        x: base_rect.origin.x + x_offset * (bounds.size.width - calculated_size.width),
        y: base_rect.origin.y,
    };

    // Pass 1 — shadow. Apple's UILabel paints the text once at
    // (origin + shadowOffset) using shadowColor, then paints the foreground
    // text on top. The shadow pass uses the same line-break/alignment
    // policy as the foreground pass so multi-line labels stay aligned.
    // See: https://developer.apple.com/documentation/uikit/uilabel/1620539-shadowcolor
    if shadow_color != nil {
        let (sr, sg, sb, sa) = ui_color::get_rgba(&env.objc, shadow_color);
        CGContextSetRGBFillColor(env, context, sr, sg, sb, sa);

        let shadow_point = CGPoint {
            x: base_point.x + shadow_offset.width,
            y: base_point.y + shadow_offset.height,
        };
        let shadow_rect = CGRect {
            origin: CGPoint {
                x: base_rect.origin.x + shadow_offset.width,
                y: base_rect.origin.y + shadow_offset.height,
            },
            size: base_rect.size,
        };

        if single_line {
            let _size: CGSize = msg![env; text drawAtPoint:shadow_point withFont:font];
        } else {
            let _size: CGSize = msg![env; text drawInRect:shadow_rect
                             withFont:font
                        lineBreakMode:line_break_mode
                            alignment:text_alignment];
        }
    }

    // Pass 2 — foreground text in the label's textColor.
    let (r, g, b, a) = ui_color::get_rgba(&env.objc, text_color);
    CGContextSetRGBFillColor(env, context, r, g, b, a);

    if single_line {
        let _size: CGSize = msg![env; text drawAtPoint:base_point withFont:font];
    } else {
        let _size: CGSize = msg![env; text drawInRect:base_rect
                         withFont:font
                    lineBreakMode:line_break_mode
                        alignment:text_alignment];
    }
}

@end

};
