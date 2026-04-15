/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIAlertView` — custom built-in UI dialog implementation.

use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::{ns_string, NSInteger, NSUInteger};
use crate::frameworks::uikit::ui_view::ui_control::UIControlEventTouchUpInside;
use crate::frameworks::uikit::ui_font::UITextAlignmentCenter;
use crate::impl_HostObject_with_superclass;
use crate::objc::{
    id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports, NSZonePtr, SEL,
};

pub type UIAlertViewStyle = NSInteger;
pub const UIAlertViewStyleDefault:               UIAlertViewStyle = 0;
pub const UIAlertViewStyleSecureTextInput:       UIAlertViewStyle = 1;
pub const UIAlertViewStylePlainTextInput:        UIAlertViewStyle = 2;
pub const UIAlertViewStyleLoginAndPasswordInput: UIAlertViewStyle = 3;

pub struct UIAlertViewHostObject {
    pub superclass:      crate::frameworks::uikit::ui_view::UIViewHostObject,
    title:               id,
    message:             id,
    delegate:            id,
    button_titles:       id, // NSMutableArray
    cancel_button_index: NSInteger,
    visible:             bool,
    alert_view_style:    UIAlertViewStyle,
    tag:                 NSInteger,
}
impl_HostObject_with_superclass!(UIAlertViewHostObject);

impl Default for UIAlertViewHostObject {
    fn default() -> Self {
        Self {
            superclass:          Default::default(),
            title:               nil,
            message:             nil,
            delegate:            nil,
            button_titles:       nil,
            cancel_button_index: -1,
            visible:             false,
            alert_view_style:    UIAlertViewStyleDefault,
            tag:                 0,
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIAlertView: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIAlertViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithTitle:(id)title
            message:(id)message
           delegate:(id)delegate
  cancelButtonTitle:(id)cancel_title
  otherButtonTitles:(id)other_titles {

    let screen: id = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];
    let this: id = msg_super![env; this initWithFrame:bounds];

    let bg_color: id = msg_class![env; UIColor colorWithWhite:0.0 alpha:0.6];
    let _: () = msg![env; this setBackgroundColor:bg_color];

    let buttons: id = msg_class![env; NSMutableArray new];
    retain(env, title);
    retain(env, message);
    retain(env, delegate);

    {
        let host = env.objc.borrow_mut::<UIAlertViewHostObject>(this);
        host.title    = title;
        host.message  = message;
        host.delegate = delegate;
        host.button_titles = buttons;
    }

    if cancel_title != nil {
        let idx: NSUInteger = msg![env; buttons count];
        let _: () = msg![env; buttons addObject:cancel_title];
        env.objc.borrow_mut::<UIAlertViewHostObject>(this).cancel_button_index = idx as NSInteger;
    }
    if other_titles != nil {
        let _: () = msg![env; buttons addObject:other_titles];
    }
    
    this
}

- (())dealloc {
    let host = env.objc.borrow_mut::<UIAlertViewHostObject>(this);
    let title = host.title;
    let message = host.message;
    let delegate = host.delegate;
    let buttons = host.button_titles;
    host.title = nil;
    host.message = nil;
    host.delegate = nil;
    host.button_titles = nil;

    release(env, title);
    release(env, message);
    release(env, delegate);
    release(env, buttons);

    msg_super![env; this dealloc]
}

- (id)title   { env.objc.borrow::<UIAlertViewHostObject>(this).title }
- (id)message { env.objc.borrow::<UIAlertViewHostObject>(this).message }
- (id)delegate { env.objc.borrow::<UIAlertViewHostObject>(this).delegate }

- (())setTitle:(id)title {
    let old = env.objc.borrow::<UIAlertViewHostObject>(this).title;
    release(env, old); retain(env, title);
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).title = title;
}

- (())setMessage:(id)message {
    let old = env.objc.borrow::<UIAlertViewHostObject>(this).message;
    release(env, old); retain(env, message);
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).message = message;
}

- (())setDelegate:(id)delegate {
    let old = env.objc.borrow::<UIAlertViewHostObject>(this).delegate;
    release(env, old); retain(env, delegate);
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).delegate = delegate;
}

- (NSInteger)tag { env.objc.borrow::<UIAlertViewHostObject>(this).tag }
- (())setTag:(NSInteger)tag { env.objc.borrow_mut::<UIAlertViewHostObject>(this).tag = tag; }
- (UIAlertViewStyle)alertViewStyle { env.objc.borrow::<UIAlertViewHostObject>(this).alert_view_style }
- (())setAlertViewStyle:(UIAlertViewStyle)style { env.objc.borrow_mut::<UIAlertViewHostObject>(this).alert_view_style = style; }
- (bool)isVisible { env.objc.borrow::<UIAlertViewHostObject>(this).visible }

- (NSInteger)addButtonWithTitle:(id)title {
    let buttons = env.objc.borrow::<UIAlertViewHostObject>(this).button_titles;
    let idx: NSUInteger = msg![env; buttons count];
    let _: () = msg![env; buttons addObject:title];
    idx as NSInteger
}

- (NSUInteger)numberOfButtons {
    let buttons = env.objc.borrow::<UIAlertViewHostObject>(this).button_titles;
    msg![env; buttons count]
}

- (id)buttonTitleAtIndex:(NSInteger)index {
    let buttons = env.objc.borrow::<UIAlertViewHostObject>(this).button_titles;
    let count: NSUInteger = msg![env; buttons count];
    if index < 0 || index as NSUInteger >= count { return nil; }
    msg![env; buttons objectAtIndex:(index as NSUInteger)]
}

- (NSInteger)cancelButtonIndex { env.objc.borrow::<UIAlertViewHostObject>(this).cancel_button_index }
- (())setCancelButtonIndex:(NSInteger)index { env.objc.borrow_mut::<UIAlertViewHostObject>(this).cancel_button_index = index; }

- (NSInteger)firstOtherButtonIndex {
    let host    = env.objc.borrow::<UIAlertViewHostObject>(this);
    let buttons = host.button_titles;
    let cancel  = host.cancel_button_index;
    let count: NSUInteger = msg![env; buttons count];
    for i in 0..count { if i as NSInteger != cancel { return i as NSInteger; } }
    -1
}

- (id)textFieldAtIndex:(NSInteger)_index { nil }

- (())show {
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).visible = true;

    let screen: id = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];
    let _: () = msg![env; this setFrame:bounds];

    let app: id = msg_class![env; UIApplication sharedApplication];
    let window: id = msg![env; app keyWindow];
    if window != nil {
        let _: () = msg![env; window addSubview:this];
    }

    let dialog_width: f32 = 280.0;
    let padding: f32 = 15.0;

    let dialog_init_frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize { width: dialog_width, height: 100.0 }
    };
    let dialog_view: id = msg_class![env; UIView alloc];
    let dialog_view: id = msg![env; dialog_view initWithFrame:dialog_init_frame];

    let dark_gray: id = msg_class![env; UIColor colorWithRed:0.2 green:0.2 blue:0.2 alpha:0.95];
    let _: () = msg![env; dialog_view setBackgroundColor:dark_gray];

    let layer: id = msg![env; dialog_view layer];
    let _: () = msg![env; layer setCornerRadius:8.0];

    let mut current_y: f32 = padding;

    let title = env.objc.borrow::<UIAlertViewHostObject>(this).title;
    if title != nil {
        let title_label: id = msg_class![env; UILabel new];
        let title_frame = CGRect {
            origin: CGPoint { x: padding, y: current_y },
            size: CGSize { width: dialog_width - padding * 2.0, height: 24.0 }
        };
        let _: () = msg![env; title_label setFrame:title_frame];
        let font: id = msg_class![env; UIFont boldSystemFontOfSize:18.0];
        let white: id = msg_class![env; UIColor whiteColor];
        let clear: id = msg_class![env; UIColor clearColor];
        let _: () = msg![env; title_label setFont:font];
        let _: () = msg![env; title_label setTextColor:white];
        let _: () = msg![env; title_label setBackgroundColor:clear];
        let _: () = msg![env; title_label setTextAlignment:UITextAlignmentCenter];
        let _: () = msg![env; title_label setText:title];
        let _: () = msg![env; dialog_view addSubview:title_label];
        release(env, title_label);
        current_y += 34.0;
    }

    let message = env.objc.borrow::<UIAlertViewHostObject>(this).message;
    if message != nil {
        let msg_label: id = msg_class![env; UILabel new];
        let msg_frame = CGRect {
            origin: CGPoint { x: padding, y: current_y },
            size: CGSize { width: dialog_width - padding * 2.0, height: 60.0 }
        };
        let _: () = msg![env; msg_label setFrame:msg_frame];
        let font: id = msg_class![env; UIFont systemFontOfSize:14.0];
        let white: id = msg_class![env; UIColor whiteColor];
        let clear: id = msg_class![env; UIColor clearColor];
        let _: () = msg![env; msg_label setFont:font];
        let _: () = msg![env; msg_label setTextColor:white];
        let _: () = msg![env; msg_label setBackgroundColor:clear];
        let _: () = msg![env; msg_label setTextAlignment:UITextAlignmentCenter];
        let _: () = msg![env; msg_label setNumberOfLines:0];
        let _: () = msg![env; msg_label setText:message];
        let _: () = msg![env; dialog_view addSubview:msg_label];
        release(env, msg_label);
        current_y += 70.0;
    }

    let buttons = env.objc.borrow::<UIAlertViewHostObject>(this).button_titles;
    let btn_count: NSUInteger = msg![env; buttons count];

    let actual_btn_count = if btn_count == 0 { 1 } else { btn_count };
    let btn_height: f32 = 44.0;
    let btn_spacing: f32 = 10.0;

    for i in 0..actual_btn_count {
        let btn_title = if btn_count == 0 {
            ns_string::get_static_str(env, "OK")
        } else {
            msg![env; buttons objectAtIndex:i]
        };

        let btn: id = msg_class![env; UIButton buttonWithType:1];
        let btn_frame = CGRect {
            origin: CGPoint { x: padding, y: current_y },
            size: CGSize { width: dialog_width - padding * 2.0, height: btn_height }
        };
        let _: () = msg![env; btn setFrame:btn_frame];
        let _: () = msg![env; btn setTitle:btn_title forState:0];
        let _: () = msg![env; btn setTag:(i as NSInteger)];

        let sel: SEL = env.objc.register_host_selector("_buttonClicked:".to_string(), &mut env.mem);
        let _: () = msg![env; btn addTarget:this action:sel forControlEvents:UIControlEventTouchUpInside];

        let _: () = msg![env; dialog_view addSubview:btn];
        current_y += btn_height + btn_spacing;
    }

    let dialog_height = current_y + padding;
    let final_dialog_frame = CGRect {
        origin: CGPoint {
            x: (bounds.size.width - dialog_width) / 2.0,
            y: (bounds.size.height - dialog_height) / 2.0,
        },
        size: CGSize { width: dialog_width, height: dialog_height }
    };
    let _: () = msg![env; dialog_view setFrame:final_dialog_frame];

    let _: () = msg![env; this addSubview:dialog_view];
    release(env, dialog_view);
}

- (())_buttonClicked:(id)sender {
    let idx: NSInteger = msg![env; sender tag];
    let _: () = msg![env; this dismissWithClickedButtonIndex:idx animated:false];
}

- (())dismissWithClickedButtonIndex:(NSInteger)button_index animated:(bool)_animated {
    env.objc.borrow_mut::<UIAlertViewHostObject>(this).visible = false;
    let _: () = msg![env; this removeFromSuperview];

    let delegate = env.objc.borrow::<UIAlertViewHostObject>(this).delegate;
    if delegate != nil {
        if let Some(sel) = env.objc.lookup_selector("alertView:clickedButtonAtIndex:") {
            let responds: bool = msg![env; delegate respondsToSelector:sel];
            if responds { let _: () = msg![env; delegate alertView:this clickedButtonAtIndex:button_index]; }
        }
        if let Some(sel) = env.objc.lookup_selector("alertView:willDismissWithButtonIndex:") {
            let responds: bool = msg![env; delegate respondsToSelector:sel];
            if responds { let _: () = msg![env; delegate alertView:this willDismissWithButtonIndex:button_index]; }
        }
        if let Some(sel) = env.objc.lookup_selector("alertView:didDismissWithButtonIndex:") {
            let responds: bool = msg![env; delegate respondsToSelector:sel];
            if responds { let _: () = msg![env; delegate alertView:this didDismissWithButtonIndex:button_index]; }
        }
    }
}

- (id)description {
    let (title, visible) = { let h = env.objc.borrow::<UIAlertViewHostObject>(this); (h.title, h.visible) };
    let title_str = if title != nil { ns_string::to_rust_string(env, title).into_owned() } else { "(nil)".into() };
    let s = format!("<UIAlertView: {:?}; title={:?}; visible={}>", this, title_str, visible);
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};
            
