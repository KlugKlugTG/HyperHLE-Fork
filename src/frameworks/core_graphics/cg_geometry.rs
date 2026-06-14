/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGGeometry.h` (`CGPoint`, `CGSize`, `CGRect`, etc)
//!
//! See also [crate::frameworks::uikit::ui_geometry].

use std::ops::{Add, Mul, Sub};

use super::CGFloat;
use crate::abi::{impl_GuestRet_for_large_struct, GuestArg};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_foundation::CFTypeRef;
use crate::mem::SafeRead;
use crate::objc::{id, msg, msg_class, nil};
use crate::Environment;

fn parse_tuple(s: &str) -> Result<(f32, f32), ()> {
    let (a, b) = s.split_once(", ").ok_or(())?;
    Ok((a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?))
}

// =========================================================================
// MARK: - CGPoint
// =========================================================================

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct CGPoint {
    pub x: CGFloat,
    pub y: CGFloat,
}
unsafe impl SafeRead for CGPoint {}
impl_GuestRet_for_large_struct!(CGPoint);
impl GuestArg for CGPoint {
    const REG_COUNT: usize = 2;
    fn from_regs(regs: &[u32]) -> Self {
        CGPoint {
            x: GuestArg::from_regs(&regs[0..1]),
            y: GuestArg::from_regs(&regs[1..2]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.x.to_regs(&mut regs[0..1]);
        self.y.to_regs(&mut regs[1..2]);
    }
}
impl std::str::FromStr for CGPoint {
    type Err = ();
    fn from_str(s: &str) -> Result<CGPoint, ()> {
        let s = s.strip_prefix('{').ok_or(())?.strip_suffix('}').ok_or(())?;
        let (x, y) = parse_tuple(s)?;
        Ok(CGPoint { x, y })
    }
}
impl std::fmt::Display for CGPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let &CGPoint { x, y } = self;
        write!(f, "{{{x}, {y}}}")
    }
}
impl Mul<f32> for CGPoint {
    type Output = CGPoint;
    fn mul(self, rhs: f32) -> Self::Output {
        CGPoint {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}
impl Add<CGPoint> for CGPoint {
    type Output = CGPoint;
    fn add(self, rhs: CGPoint) -> Self::Output {
        CGPoint {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}
impl Sub<CGPoint> for CGPoint {
    type Output = CGPoint;
    fn sub(self, rhs: CGPoint) -> Self::Output {
        CGPoint {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

pub const CGPointZero: CGPoint = CGPoint { x: 0.0, y: 0.0 };

fn CGPointEqualToPoint(_env: &mut Environment, a: CGPoint, b: CGPoint) -> bool {
    a == b
}

fn CGPointMake(_env: &mut Environment, x: CGFloat, y: CGFloat) -> CGPoint {
    CGPoint { x, y }
}

// =========================================================================
// MARK: - CGSize
// =========================================================================

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct CGSize {
    pub width: CGFloat,
    pub height: CGFloat,
}
unsafe impl SafeRead for CGSize {}
impl_GuestRet_for_large_struct!(CGSize);
impl GuestArg for CGSize {
    const REG_COUNT: usize = 2;
    fn from_regs(regs: &[u32]) -> Self {
        CGSize {
            width: GuestArg::from_regs(&regs[0..1]),
            height: GuestArg::from_regs(&regs[1..2]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.width.to_regs(&mut regs[0..1]);
        self.height.to_regs(&mut regs[1..2]);
    }
}
impl std::str::FromStr for CGSize {
    type Err = ();
    fn from_str(s: &str) -> Result<CGSize, ()> {
        let s = s.strip_prefix('{').ok_or(())?.strip_suffix('}').ok_or(())?;
        let (w, h) = parse_tuple(s)?;
        Ok(CGSize {
            width: w,
            height: h,
        })
    }
}
impl std::fmt::Display for CGSize {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let &CGSize { width, height } = self;
        write!(f, "{{{width}, {height}}}")
    }
}
impl Mul<f32> for CGSize {
    type Output = CGSize;
    fn mul(self, rhs: f32) -> Self::Output {
        CGSize {
            width: self.width * rhs,
            height: self.height * rhs,
        }
    }
}
impl Add<CGSize> for CGSize {
    type Output = CGSize;
    fn add(self, rhs: CGSize) -> Self::Output {
        CGSize {
            width: self.width + rhs.width,
            height: self.height + rhs.height,
        }
    }
}
impl Sub<CGSize> for CGSize {
    type Output = CGSize;
    fn sub(self, rhs: CGSize) -> Self::Output {
        CGSize {
            width: self.width - rhs.width,
            height: self.height - rhs.height,
        }
    }
}

pub const CGSizeZero: CGSize = CGSize {
    width: 0.0,
    height: 0.0,
};

fn CGSizeEqualToSize(_env: &mut Environment, a: CGSize, b: CGSize) -> bool {
    a == b
}

fn CGSizeMake(_env: &mut Environment, width: CGFloat, height: CGFloat) -> CGSize {
    CGSize { width, height }
}

// =========================================================================
// MARK: - CGRect
// =========================================================================

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}
unsafe impl SafeRead for CGRect {}
impl_GuestRet_for_large_struct!(CGRect);
impl GuestArg for CGRect {
    const REG_COUNT: usize = 4;
    fn from_regs(regs: &[u32]) -> Self {
        CGRect {
            origin: GuestArg::from_regs(&regs[0..2]),
            size: GuestArg::from_regs(&regs[2..4]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.origin.to_regs(&mut regs[0..2]);
        self.size.to_regs(&mut regs[2..4]);
    }
}
impl std::str::FromStr for CGRect {
    type Err = ();
    fn from_str(s: &str) -> Result<CGRect, ()> {
        let s = s
            .strip_prefix("{{")
            .ok_or(())?
            .strip_suffix("}}")
            .ok_or(())?;
        let (a, b) = s.split_once("}, {").ok_or(())?;
        let (x, y) = parse_tuple(a)?;
        let (width, height) = parse_tuple(b)?;
        Ok(CGRect {
            origin: CGPoint { x, y },
            size: CGSize { width, height },
        })
    }
}
impl std::fmt::Display for CGRect {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let &CGRect { origin, size } = self;
        write!(f, "{{{origin}, {size}}}")
    }
}
impl Mul<f32> for CGRect {
    type Output = CGRect;
    fn mul(self, rhs: f32) -> Self::Output {
        CGRect {
            origin: self.origin * rhs,
            size: self.size * rhs,
        }
    }
}
impl Add<CGRect> for CGRect {
    type Output = CGRect;
    fn add(self, rhs: CGRect) -> Self::Output {
        CGRect {
            origin: self.origin + rhs.origin,
            size: self.size + rhs.size,
        }
    }
}
impl Sub<CGRect> for CGRect {
    type Output = CGRect;
    fn sub(self, rhs: CGRect) -> Self::Output {
        CGRect {
            origin: self.origin - rhs.origin,
            size: self.size - rhs.size,
        }
    }
}

pub const CGRectZero: CGRect = CGRect {
    origin: CGPointZero,
    size: CGSizeZero,
};

pub const CGRectNull: CGRect = CGRect {
    origin: CGPoint {
        x: f32::INFINITY,
        y: f32::INFINITY,
    },
    size: CGSizeZero,
};

pub const CGRectInfinite: CGRect = CGRect {
    origin: CGPoint {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
    },
    size: CGSize {
        width: f32::INFINITY,
        height: f32::INFINITY,
    },
};

// =========================================================================
// MARK: - CGRect predicate functions
// =========================================================================

fn CGRectEqualToRect(_env: &mut Environment, a: CGRect, b: CGRect) -> bool {
    a == b
}

fn CGRectIsNull(_env: &mut Environment, rect: CGRect) -> bool {
    rect == CGRectNull
}

fn CGRectIsEmpty(_env: &mut Environment, rect: CGRect) -> bool {
    rect == CGRectNull || rect.size.width == 0.0 || rect.size.height == 0.0
}

fn CGRectIsInfinite(_env: &mut Environment, rect: CGRect) -> bool {
    rect.origin.x.is_infinite()
        || rect.origin.y.is_infinite()
        || rect.size.width.is_infinite()
        || rect.size.height.is_infinite()
}

fn CGRectContainsPoint(_env: &mut Environment, rect: CGRect, point: CGPoint) -> bool {
    rect.origin.x <= point.x
        && rect.origin.x + rect.size.width > point.x
        && rect.origin.y <= point.y
        && rect.origin.y + rect.size.height > point.y
}

// MARK: - CGPoint dictionary representation

fn CGPointCreateDictionaryRepresentation(env: &mut Environment, point: CGPoint) -> CFTypeRef {
    let dict: id = msg_class![env; NSMutableDictionary new];
    let x_key = crate::frameworks::foundation::ns_string::get_static_str(env, "X");
    let y_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Y");
    let x_num: id = msg_class![env; NSNumber numberWithDouble:(point.x as f64)];
    let y_num: id = msg_class![env; NSNumber numberWithDouble:(point.y as f64)];
    () = msg![env; dict setObject:x_num forKey:x_key];
    () = msg![env; dict setObject:y_num forKey:y_key];
    crate::objc::autorelease(env, dict)
}

fn CGPointMakeWithDictionaryRepresentation(
    env: &mut Environment,
    dict: CFTypeRef,
    point: crate::mem::MutPtr<CGPoint>,
) -> bool {
    if dict.is_null() || point.is_null() {
        return false;
    }
    let x_key = crate::frameworks::foundation::ns_string::get_static_str(env, "X");
    let y_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Y");
    let x_val: id = msg![env; dict objectForKey:x_key];
    let y_val: id = msg![env; dict objectForKey:y_key];
    if x_val == nil || y_val == nil {
        return false;
    }
    let x: f64 = msg![env; x_val doubleValue];
    let y: f64 = msg![env; y_val doubleValue];
    env.mem.write(
        point,
        CGPoint {
            x: x as _,
            y: y as _,
        },
    );
    true
}

// MARK: - CGSize dictionary representation

fn CGSizeCreateDictionaryRepresentation(env: &mut Environment, size: CGSize) -> CFTypeRef {
    let dict: id = msg_class![env; NSMutableDictionary new];
    let w_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Width");
    let h_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Height");
    let w_num: id = msg_class![env; NSNumber numberWithDouble:(size.width  as f64)];
    let h_num: id = msg_class![env; NSNumber numberWithDouble:(size.height as f64)];
    () = msg![env; dict setObject:w_num forKey:w_key];
    () = msg![env; dict setObject:h_num forKey:h_key];
    crate::objc::autorelease(env, dict)
}

fn CGSizeMakeWithDictionaryRepresentation(
    env: &mut Environment,
    dict: CFTypeRef,
    size: crate::mem::MutPtr<CGSize>,
) -> bool {
    if dict.is_null() || size.is_null() {
        return false;
    }
    let w_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Width");
    let h_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Height");
    let w_val: id = msg![env; dict objectForKey:w_key];
    let h_val: id = msg![env; dict objectForKey:h_key];
    if w_val == nil || h_val == nil {
        return false;
    }
    let w: f64 = msg![env; w_val doubleValue];
    let h: f64 = msg![env; h_val doubleValue];
    env.mem.write(
        size,
        CGSize {
            width: w as _,
            height: h as _,
        },
    );
    true
}

// MARK: - CGRect dictionary representation

fn CGRectCreateDictionaryRepresentation(env: &mut Environment, rect: CGRect) -> CFTypeRef {
    let dict: id = msg_class![env; NSMutableDictionary new];
    let x_key = crate::frameworks::foundation::ns_string::get_static_str(env, "X");
    let y_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Y");
    let w_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Width");
    let h_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Height");
    let x_num: id = msg_class![env; NSNumber numberWithDouble:(rect.origin.x    as f64)];
    let y_num: id = msg_class![env; NSNumber numberWithDouble:(rect.origin.y    as f64)];
    let w_num: id = msg_class![env; NSNumber numberWithDouble:(rect.size.width  as f64)];
    let h_num: id = msg_class![env; NSNumber numberWithDouble:(rect.size.height as f64)];
    () = msg![env; dict setObject:x_num forKey:x_key];
    () = msg![env; dict setObject:y_num forKey:y_key];
    () = msg![env; dict setObject:w_num forKey:w_key];
    () = msg![env; dict setObject:h_num forKey:h_key];
    crate::objc::autorelease(env, dict)
}

fn CGRectMakeWithDictionaryRepresentation(
    env: &mut Environment,
    dict: CFTypeRef,
    rect: crate::mem::MutPtr<CGRect>,
) -> bool {
    if dict.is_null() || rect.is_null() {
        return false;
    }
    let x_key = crate::frameworks::foundation::ns_string::get_static_str(env, "X");
    let y_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Y");
    let w_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Width");
    let h_key = crate::frameworks::foundation::ns_string::get_static_str(env, "Height");
    let x_val: id = msg![env; dict objectForKey:x_key];
    let y_val: id = msg![env; dict objectForKey:y_key];
    let w_val: id = msg![env; dict objectForKey:w_key];
    let h_val: id = msg![env; dict objectForKey:h_key];
    if x_val == nil || y_val == nil || w_val == nil || h_val == nil {
        return false;
    }
    let x: f64 = msg![env; x_val doubleValue];
    let y: f64 = msg![env; y_val doubleValue];
    let w: f64 = msg![env; w_val doubleValue];
    let h: f64 = msg![env; h_val doubleValue];
    env.mem.write(
        rect,
        CGRect {
            origin: CGPoint {
                x: x as _,
                y: y as _,
            },
            size: CGSize {
                width: w as _,
                height: h as _,
            },
        },
    );
    true
}

// MARK: - CGAffineTransform dictionary representation

fn CGAffineTransformCreateDictionaryRepresentation(
    env: &mut Environment,
    transform: crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform,
) -> CFTypeRef {
    let dict: id = msg_class![env; NSMutableDictionary new];
    let keys = ["a", "b", "c", "d", "tx", "ty"];
    let vals = [
        transform.a,
        transform.b,
        transform.c,
        transform.d,
        transform.tx,
        transform.ty,
    ];
    for (k, v) in keys.iter().zip(vals.iter()) {
        let ns_key = crate::frameworks::foundation::ns_string::get_static_str(env, k);
        let ns_val: id = msg_class![env; NSNumber numberWithDouble:(*v as f64)];
        () = msg![env; dict setObject:ns_val forKey:ns_key];
    }
    crate::objc::autorelease(env, dict)
}

fn CGAffineTransformMakeWithDictionaryRepresentation(
    env: &mut Environment,
    dict: CFTypeRef,
    transform: crate::mem::MutPtr<
        crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform,
    >,
) -> bool {
    use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
    if dict.is_null() || transform.is_null() {
        return false;
    }
    let keys = ["a", "b", "c", "d", "tx", "ty"];
    let mut vals = [0f64; 6];
    for (i, k) in keys.iter().enumerate() {
        let ns_key = crate::frameworks::foundation::ns_string::get_static_str(env, k);
        let val: id = msg![env; dict objectForKey:ns_key];
        if val == nil {
            return false;
        }
        vals[i] = msg![env; val doubleValue];
    }
    env.mem.write(
        transform,
        CGAffineTransform {
            a: vals[0] as _,
            b: vals[1] as _,
            c: vals[2] as _,
            d: vals[3] as _,
            tx: vals[4] as _,
            ty: vals[5] as _,
        },
    );
    true
}

fn CGRectContainsRect(_env: &mut Environment, outer: CGRect, inner: CGRect) -> bool {
    outer.origin.x <= inner.origin.x
        && outer.origin.y <= inner.origin.y
        && outer.origin.x + outer.size.width >= inner.origin.x + inner.size.width
        && outer.origin.y + outer.size.height >= inner.origin.y + inner.size.height
}

fn CGRectIntersectsRect(_env: &mut Environment, rect1: CGRect, rect2: CGRect) -> bool {
    rect1.origin.x.max(rect2.origin.x)
        < (rect1.origin.x + rect1.size.width).min(rect2.origin.x + rect2.size.width)
        && rect1.origin.y.max(rect2.origin.y)
            < (rect1.origin.y + rect1.size.height).min(rect2.origin.y + rect2.size.height)
}

// =========================================================================
// MARK: - CGRect geometry operations
// =========================================================================

fn CGRectMake(
    _env: &mut Environment,
    x: CGFloat,
    y: CGFloat,
    width: CGFloat,
    height: CGFloat,
) -> CGRect {
    CGRect {
        origin: CGPoint { x, y },
        size: CGSize { width, height },
    }
}

fn CGRectGetMinX(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.x
}
fn CGRectGetMidX(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.x + rect.size.width / 2.0
}
fn CGRectGetMaxX(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.x + rect.size.width
}
fn CGRectGetMinY(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.y
}
fn CGRectGetMidY(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.y + rect.size.height / 2.0
}
fn CGRectGetMaxY(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.origin.y + rect.size.height
}
fn CGRectGetWidth(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.size.width
}
fn CGRectGetHeight(_env: &mut Environment, rect: CGRect) -> CGFloat {
    rect.size.height
}

fn CGRectOffset(_env: &mut Environment, rect: CGRect, dx: CGFloat, dy: CGFloat) -> CGRect {
    if rect == CGRectNull {
        return CGRectNull;
    }
    CGRect {
        origin: CGPoint {
            x: rect.origin.x + dx,
            y: rect.origin.y + dy,
        },
        size: rect.size,
    }
}

fn CGRectInset(_env: &mut Environment, rect: CGRect, dx: CGFloat, dy: CGFloat) -> CGRect {
    if rect == CGRectNull {
        return CGRectNull;
    }
    let new_w = rect.size.width - 2.0 * dx;
    let new_h = rect.size.height - 2.0 * dy;
    if new_w < 0.0 || new_h < 0.0 {
        return CGRectNull;
    }
    CGRect {
        origin: CGPoint {
            x: rect.origin.x + dx,
            y: rect.origin.y + dy,
        },
        size: CGSize {
            width: new_w,
            height: new_h,
        },
    }
}

fn CGRectIntersection(_env: &mut Environment, r1: CGRect, r2: CGRect) -> CGRect {
    if r1 == CGRectNull || r2 == CGRectNull {
        return CGRectNull;
    }
    let x1 = r1.origin.x.max(r2.origin.x);
    let y1 = r1.origin.y.max(r2.origin.y);
    let x2 = (r1.origin.x + r1.size.width).min(r2.origin.x + r2.size.width);
    let y2 = (r1.origin.y + r1.size.height).min(r2.origin.y + r2.size.height);
    if x2 <= x1 || y2 <= y1 {
        return CGRectNull;
    }
    CGRect {
        origin: CGPoint { x: x1, y: y1 },
        size: CGSize {
            width: x2 - x1,
            height: y2 - y1,
        },
    }
}

fn CGRectUnion(_env: &mut Environment, r1: CGRect, r2: CGRect) -> CGRect {
    // CGRectNull is the identity element for union.
    if r1 == CGRectNull {
        return r2;
    }
    if r2 == CGRectNull {
        return r1;
    }
    let x1 = r1.origin.x.min(r2.origin.x);
    let y1 = r1.origin.y.min(r2.origin.y);
    let x2 = (r1.origin.x + r1.size.width).max(r2.origin.x + r2.size.width);
    let y2 = (r1.origin.y + r1.size.height).max(r2.origin.y + r2.size.height);
    CGRect {
        origin: CGPoint { x: x1, y: y1 },
        size: CGSize {
            width: x2 - x1,
            height: y2 - y1,
        },
    }
}

/// `CGRectDivide` splits `rect` into two rectangles.
/// `amount` units are taken from the edge indicated by `edge` (CGRectEdge).
/// CGRectEdge: 0=MinX, 1=MinY, 2=MaxX, 3=MaxY
fn CGRectDivide(
    env: &mut Environment,
    rect: CGRect,
    slice_out: crate::mem::MutPtr<CGRect>,
    remainder_out: crate::mem::MutPtr<CGRect>,
    amount: CGFloat,
    edge: u32,
) {
    let (slice, remainder) = rect_divide(rect, amount, edge);
    if !slice_out.is_null() {
        env.mem.write(slice_out, slice);
    }
    if !remainder_out.is_null() {
        env.mem.write(remainder_out, remainder);
    }
}

fn rect_divide(rect: CGRect, amount: CGFloat, edge: u32) -> (CGRect, CGRect) {
    let CGRect {
        origin: CGPoint { x, y },
        size: CGSize { width, height },
    } = rect;
    match edge {
        0 => {
            // MinXEdge
            let amt = amount.min(width);
            (
                CGRect {
                    origin: CGPoint { x, y },
                    size: CGSize { width: amt, height },
                },
                CGRect {
                    origin: CGPoint { x: x + amt, y },
                    size: CGSize {
                        width: width - amt,
                        height,
                    },
                },
            )
        }
        1 => {
            // MinYEdge
            let amt = amount.min(height);
            (
                CGRect {
                    origin: CGPoint { x, y },
                    size: CGSize { width, height: amt },
                },
                CGRect {
                    origin: CGPoint { x, y: y + amt },
                    size: CGSize {
                        width,
                        height: height - amt,
                    },
                },
            )
        }
        2 => {
            // MaxXEdge
            let amt = amount.min(width);
            (
                CGRect {
                    origin: CGPoint {
                        x: x + width - amt,
                        y,
                    },
                    size: CGSize { width: amt, height },
                },
                CGRect {
                    origin: CGPoint { x, y },
                    size: CGSize {
                        width: width - amt,
                        height,
                    },
                },
            )
        }
        3 => {
            // MaxYEdge
            let amt = amount.min(height);
            (
                CGRect {
                    origin: CGPoint {
                        x,
                        y: y + height - amt,
                    },
                    size: CGSize { width, height: amt },
                },
                CGRect {
                    origin: CGPoint { x, y },
                    size: CGSize {
                        width,
                        height: height - amt,
                    },
                },
            )
        }
        _ => (CGRectNull, rect),
    }
}

/// `CGRect CGRectStandardize(CGRect rect)`
///
/// Returns a rectangle with a positive width and height by flipping the origin
/// if either dimension is negative.
fn CGRectStandardize(_env: &mut Environment, rect: CGRect) -> CGRect {
    if rect == CGRectNull {
        return CGRectNull;
    }
    let mut r = rect;
    if r.size.width < 0.0 {
        r.origin.x += r.size.width;
        r.size.width = -r.size.width;
    }
    if r.size.height < 0.0 {
        r.origin.y += r.size.height;
        r.size.height = -r.size.height;
    }
    r
}

/// `CGRect CGRectIntegral(CGRect rect)`
///
/// Returns the smallest rectangle with integer coordinates that contains the
/// given rectangle.
fn CGRectIntegral(_env: &mut Environment, rect: CGRect) -> CGRect {
    if rect == CGRectNull {
        return CGRectNull;
    }
    let x = rect.origin.x.floor();
    let y = rect.origin.y.floor();
    let max_x = (rect.origin.x + rect.size.width).ceil();
    let max_y = (rect.origin.y + rect.size.height).ceil();
    CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: max_x - x,
            height: max_y - y,
        },
    }
}

// =========================================================================
// MARK: - CGVector (iOS 7+)
// =========================================================================

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct CGVector {
    pub dx: CGFloat,
    pub dy: CGFloat,
}
unsafe impl SafeRead for CGVector {}
impl_GuestRet_for_large_struct!(CGVector);
impl GuestArg for CGVector {
    const REG_COUNT: usize = 2;
    fn from_regs(regs: &[u32]) -> Self {
        CGVector {
            dx: GuestArg::from_regs(&regs[0..1]),
            dy: GuestArg::from_regs(&regs[1..2]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.dx.to_regs(&mut regs[0..1]);
        self.dy.to_regs(&mut regs[1..2]);
    }
}

fn CGVectorMake(_env: &mut Environment, dx: CGFloat, dy: CGFloat) -> CGVector {
    CGVector { dx, dy }
}

pub const FUNCTIONS: FunctionExports = &[
    // CGPoint
    export_c_func!(CGPointMake(_, _)),
    export_c_func!(CGPointEqualToPoint(_, _)),
    // CGSize
    export_c_func!(CGSizeMake(_, _)),
    export_c_func!(CGSizeEqualToSize(_, _)),
    // CGRect construction
    export_c_func!(CGRectMake(_, _, _, _)),
    export_c_func!(CGRectEqualToRect(_, _)),
    // CGRect predicates
    export_c_func!(CGRectIsNull(_)),
    export_c_func!(CGRectIsEmpty(_)),
    export_c_func!(CGRectIsInfinite(_)),
    export_c_func!(CGRectContainsPoint(_, _)),
    export_c_func!(CGRectContainsRect(_, _)),
    export_c_func!(CGRectIntersectsRect(_, _)),
    // CGRect accessors
    export_c_func!(CGRectGetMinX(_)),
    export_c_func!(CGRectGetMidX(_)),
    export_c_func!(CGRectGetMaxX(_)),
    export_c_func!(CGRectGetMinY(_)),
    export_c_func!(CGRectGetMidY(_)),
    export_c_func!(CGRectGetMaxY(_)),
    export_c_func!(CGRectGetWidth(_)),
    export_c_func!(CGRectGetHeight(_)),
    // CGRect operations
    export_c_func!(CGRectOffset(_, _, _)),
    export_c_func!(CGRectInset(_, _, _)),
    export_c_func!(CGRectIntersection(_, _)),
    export_c_func!(CGRectUnion(_, _)),
    export_c_func!(CGRectStandardize(_)),
    export_c_func!(CGRectIntegral(_)),
    export_c_func!(CGRectDivide(_, _, _, _, _)),
    // CGVector
    export_c_func!(CGVectorMake(_, _)),
    export_c_func!(CGPointCreateDictionaryRepresentation(_)),
    export_c_func!(CGPointMakeWithDictionaryRepresentation(_, _)),
    export_c_func!(CGSizeCreateDictionaryRepresentation(_)),
    export_c_func!(CGSizeMakeWithDictionaryRepresentation(_, _)),
    export_c_func!(CGRectCreateDictionaryRepresentation(_)),
    export_c_func!(CGRectMakeWithDictionaryRepresentation(_, _)),
    export_c_func!(CGAffineTransformCreateDictionaryRepresentation(_)),
    export_c_func!(CGAffineTransformMakeWithDictionaryRepresentation(_, _)),
];

pub const CONSTANTS: ConstantExports = &[
    (
        "_CGSizeZero",
        HostConstant::Custom(|env| env.mem.alloc_and_write(CGSizeZero).cast().cast_const()),
    ),
    (
        "_CGPointZero",
        HostConstant::Custom(|env| env.mem.alloc_and_write(CGPointZero).cast().cast_const()),
    ),
    (
        "_CGRectZero",
        HostConstant::Custom(|env| env.mem.alloc_and_write(CGRectZero).cast().cast_const()),
    ),
    (
        "_CGRectNull",
        HostConstant::Custom(|env| env.mem.alloc_and_write(CGRectNull).cast().cast_const()),
    ),
    (
        "_CGRectInfinite",
        HostConstant::Custom(|env| env.mem.alloc_and_write(CGRectInfinite).cast().cast_const()),
    ),
];
