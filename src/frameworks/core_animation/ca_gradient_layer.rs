/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CAGradientLayer`.
//!
//! Chrome (and other apps) create gradient layers via `+[CAGradientLayer
//! layer]` for fading toolbar / scroll-edge effects. touchHLE's compositor
//! doesn't render gradients yet, but the layer must be a real `CALayer`
//! subclass so it composites (showing its background color) and so property
//! accessors round-trip instead of warning "class is unimplemented".

use super::ca_layer::{CALayerHostObject, kCAGravityResize, kCAFilterLinear};
use crate::frameworks::core_graphics::{
    cg_affine_transform::CGAffineTransformIdentity, CGFloat, CGPoint, CGRect, CGSize,
};
use crate::frameworks::core_animation::ca_transform3d::CATransform3DIdentity;
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::objc::{id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports, NSZonePtr};
use crate::Environment;
use std::collections::{HashMap, HashSet};

/// Gradient properties stored outside `CALayerHostObject` so we don't have
/// to touch CALayer's allocator. Retained ids are released on drop.
#[derive(Default)]
pub struct State {
    /// `CAGradientLayer*` -> (colors: `NSArray*` or nil, start: (x, y),
    /// end: (x, y), type_name)
    gradients: HashMap<id, GradientProps>,
}

impl State {
    pub fn get(&self, k: &id) -> Option<&GradientProps> {
        self.gradients.get(k)
    }
    pub fn entry(&mut self, k: id) -> std::collections::hash_map::Entry<'_, id, GradientProps> {
        self.gradients.entry(k)
    }
    pub fn remove(&mut self, k: &id) -> Option<GradientProps> {
        self.gradients.remove(k)
    }
}

#[derive(Clone, Copy)]
pub struct GradientProps {
    /// Colors of the gradient (`NSArray*` or nil).
    pub colors: id,
    /// Normalized start/end points of the gradient axis.
    pub start: (CGFloat, CGFloat),
    pub end: (CGFloat, CGFloat),
}

impl Default for GradientProps {
    fn default() -> Self {
        // Apple's defaults: vertical gradient from top (0.5, 0.0) to
        // bottom (0.5, 1.0).
        GradientProps {
            colors: crate::objc::nil,
            start: (0.5, 0.0),
            end: (0.5, 1.0),
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CAGradientLayer: CALayer

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(CALayerHostObject {
        metal_device: nil,
        metal_pixel_format: 0,
        metal_drawable_size: CGSize { width: 0.0, height: 0.0 },
        metal_framebuffer_only: false,
        delegate: nil,
        sublayers: Vec::new(),
        superlayer: nil,
        bounds: CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: 0.0, height: 0.0 } },
        position: CGPoint { x: 0.0, y: 0.0 },
        z_position: 0.0,
        anchor_point: CGPoint { x: 0.5, y: 0.5 },
        affine_transform: CGAffineTransformIdentity,
        transform_3d: CATransform3DIdentity,
        sublayer_transform: CATransform3DIdentity,
        hidden: false,
        opaque: false,
        opacity: 1.0,
        background_color: None,
        background_pattern_cg_image: nil,
        background_pattern_gles_texture: None,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: None,
        needs_display: false,
        needs_display_on_bounds_change: false,
        contents: nil,
        drawable_properties: nil,
        presented_pixels: None,
        cg_context: None,
        gles_texture: None,
        gles_texture_is_up_to_date: false,
        animations: HashMap::new(),
        anonymous_animations: HashSet::new(),
        name: None,
        mask: nil,
        contents_gravity: kCAGravityResize.to_owned(),
        contents_rect: CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: 1.0, height: 1.0 } },
        edge_antialiasing_mask: 0,
        minification_filter: kCAFilterLinear.to_owned(),
        magnification_filter: kCAFilterLinear.to_owned(),
        minification_filter_bias: 0.0,
        use_implicit_animations: true,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// CAGradientLayer's designated creation path (also used by the older
// `+[CAGradientLayer layer]` convenience constructor inherited from CALayer).
+ (id)layer {
    let layer: id = msg_class![env; CAGradientLayer alloc];
    let layer: id = msg![env; layer init];
    log_dbg!("CAGradientLayer created ({:#x})", layer.to_bits());
    layer
}

- (id)colors { // NSArray* of CGColorRef/UIColor
    let props = env.framework_state.core_animation.gradients.get(&this);
    match props {
        Some(p) => p.colors,
        None => nil,
    }
}

- (())setColors:(id)colors { // NSArray*
    retain(env, colors);
    let state = &mut env.framework_state.core_animation;
    let entry = state.gradients.entry(this).or_default();
    let old = std::mem::replace(&mut entry.colors, colors);
    release(env, old);
}

- (CGPoint)startPoint {
    env.framework_state
        .core_animation
        .gradients
        .get(&this)
        .map(|p| CGPoint { x: p.start.0, y: p.start.1 })
        .unwrap_or(CGPoint { x: 0.5, y: 0.0 })
}

- (())setStartPoint:(CGPoint)point {
    let state = &mut env.framework_state.core_animation;
    let entry = state.gradients.entry(this).or_default();
    entry.start = (point.x, point.y);
}

- (CGPoint)endPoint {
    env.framework_state
        .core_animation
        .gradients
        .get(&this)
        .map(|p| CGPoint { x: p.end.0, y: p.end.1 })
        .unwrap_or(CGPoint { x: 0.5, y: 1.0 })
}

- (())setEndPoint:(CGPoint)point {
    let state = &mut env.framework_state.core_animation;
    let entry = state.gradients.entry(this).or_default();
    entry.end = (point.x, point.y);
}

- (id)gradientType {
    // Apple's constant kCAGradientLayerAxial has value "axial"
    get_static_str(env, "axial")
}

- (())setGradientType:(id)_type {
    // Only axial gradients are meaningful without real gradient rendering.
}

- (())dealloc {
    let old = env
        .framework_state
        .core_animation
        .gradients
        .remove(&this);
    if let Some(props) = old {
        release(env, props.colors);
    }
    msg_super![env; this dealloc]
}

@end

};
