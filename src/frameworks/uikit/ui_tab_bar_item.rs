/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UITabBarItem`.

use crate::frameworks::foundation::NSInteger;
use crate::objc::{id, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr};

// MARK: - UITabBarItem host object

#[derive(Default)]
struct UITabBarItemHostObject {
    title: id,          // NSString* — retained
    image: id,          // UIImage* — retained
    selected_image: id, // UIImage* — retained
    badge_value: id,    // NSString* — retained
    tag: NSInteger,
    enabled: bool,
}
impl HostObject for UITabBarItemHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// =========================================================================
// MARK: - UITabBarItem
// =========================================================================

@implementation UITabBarItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UITabBarItemHostObject {
        title: nil,
        image: nil,
        selected_image: nil,
        badge_value: nil,
        tag: 0,
        enabled: true,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (id)initWithCoder:(id)_coder {
    nil
}

- (id)initWithTitle:(id)title image:(id)image tag:(NSInteger)tag {
    retain(env, title);
    retain(env, image);
    {
        let host = env.objc.borrow_mut::<UITabBarItemHostObject>(this);
        host.title = title;
        host.image = image;
        host.tag = tag;
    }
    this
}

- (id)initWithTitle:(id)title image:(id)image selectedImage:(id)selected_image {
    retain(env, title);
    retain(env, image);
    retain(env, selected_image);
    {
        let host = env.objc.borrow_mut::<UITabBarItemHostObject>(this);
        host.title = title;
        host.image = image;
        host.selected_image = selected_image;
    }
    this
}

- (id)initWithTabBarSystemItem:(NSInteger)_system_item tag:(NSInteger)tag {
    // В touchHLE отрисовка системных иконок пока может быть не реализована,
    // но мы обязаны корректно сохранить tag и вернуть рабочий объект.
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).tag = tag;
    this
}

- (())dealloc {
    let host = env.objc.borrow::<UITabBarItemHostObject>(this);
    let (title, image, selected_image, badge_value) = (
        host.title, host.image, host.selected_image, host.badge_value
    );
    release(env, title);
    release(env, image);
    release(env, selected_image);
    release(env, badge_value);
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Properties

- (id)title {
    env.objc.borrow::<UITabBarItemHostObject>(this).title
}

- (())setTitle:(id)title {
    retain(env, title);
    let old = env.objc.borrow::<UITabBarItemHostObject>(this).title;
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).title = title;
    release(env, old);
}

- (id)image {
    env.objc.borrow::<UITabBarItemHostObject>(this).image
}

- (())setImage:(id)image {
    retain(env, image);
    let old = env.objc.borrow::<UITabBarItemHostObject>(this).image;
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).image = image;
    release(env, old);
}

- (id)selectedImage {
    env.objc.borrow::<UITabBarItemHostObject>(this).selected_image
}

- (())setSelectedImage:(id)image {
    retain(env, image);
    let old = env.objc.borrow::<UITabBarItemHostObject>(this).selected_image;
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).selected_image = image;
    release(env, old);
}

- (id)badgeValue {
    env.objc.borrow::<UITabBarItemHostObject>(this).badge_value
}

- (())setBadgeValue:(id)value {
    retain(env, value);
    let old = env.objc.borrow::<UITabBarItemHostObject>(this).badge_value;
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).badge_value = value;
    release(env, old);
}

- (NSInteger)tag {
    env.objc.borrow::<UITabBarItemHostObject>(this).tag
}

- (())setTag:(NSInteger)tag {
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).tag = tag;
}

- (bool)isEnabled {
    env.objc.borrow::<UITabBarItemHostObject>(this).enabled
}

- (())setEnabled:(bool)enabled {
    env.objc.borrow_mut::<UITabBarItemHostObject>(this).enabled = enabled;
}

@end

};
