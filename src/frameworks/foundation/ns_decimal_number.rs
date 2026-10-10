/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSDecimalNumber` and `NSDecimal`.

use super::ns_string::{from_rust_string, to_rust_string};
use super::ns_value::NSNumberHostObject;
use super::NSUInteger;
use crate::frameworks::foundation::{ns_string, NSInteger};
use crate::objc::{
    autorelease, id, impl_HostObject_with_superclass, msg, msg_class, nil, objc_classes, release,
    retain, ClassExports, HostObject, NSZonePtr,
};

// MARK: - Rounding modes

type NSRoundingMode = NSUInteger;
const NSRoundPlain: NSRoundingMode = 0; // round half up
const NSRoundDown: NSRoundingMode = 1;
const NSRoundUp: NSRoundingMode = 2;
const NSRoundBankers: NSRoundingMode = 3; // round half to even

// MARK: - Calculation error codes

type NSCalculationError = NSUInteger;
const NSCalculationNoError: NSCalculationError = 0;
const NSCalculationLossOfPrecision: NSCalculationError = 1;
const NSCalculationUnderflow: NSCalculationError = 2;
const NSCalculationOverflow: NSCalculationError = 3;
const NSCalculationDivideByZero: NSCalculationError = 4;

// MARK: - Host object

/// NSDecimalNumber is an NSNumber subclass, so keep the NSNumber host object
/// as its host-side superclass. Inherited NSNumber and Core Foundation methods
/// can then borrow that representation through the host-object superclass
/// chain instead of falling back to a fake wrong-type borrow.
#[derive(Default)]
struct NSDecimalNumberHostObject {
    superclass: NSNumberHostObject,
}
impl_HostObject_with_superclass!(NSDecimalNumberHostObject);

/// We approximate NSDecimal storage with an f64 for the game use-cases
/// touchHLE targets. The shared NSNumber representation stores it as Double.
fn decimal_number_value(env: &crate::Environment, number: id) -> f64 {
    env.objc.borrow::<NSNumberHostObject>(number).as_double()
}

fn set_decimal_number_value(env: &mut crate::Environment, number: id, value: f64) {
    *env.objc.borrow_mut::<NSNumberHostObject>(number) = NSNumberHostObject::Double(value);
}

// MARK: - NSDecimalNumberHandler host object

#[derive(Default)]
struct NSDecimalNumberHandlerHostObject {
    rounding_mode: NSRoundingMode,
    scale: i16,
    raise_on_exactness: bool,
    raise_on_overflow: bool,
    raise_on_underflow: bool,
    raise_on_divide_by_zero: bool,
}
impl HostObject for NSDecimalNumberHandlerHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// =========================================================================
// NSDecimalNumber
// =========================================================================

@implementation NSDecimalNumber: NSNumber

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSDecimalNumberHostObject {
        superclass: NSNumberHostObject::Double(0.0),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Sentinel values

+ (id)zero {
    let n: id = msg![env; this alloc];
    set_decimal_number_value(env, n, 0.0);
    autorelease(env, n)
}

+ (id)one {
    let n: id = msg![env; this alloc];
    set_decimal_number_value(env, n, 1.0);
    autorelease(env, n)
}

+ (id)minimumDecimalNumber {
    let n: id = msg![env; this alloc];
    set_decimal_number_value(env, n, f64::MIN);
    autorelease(env, n)
}

+ (id)maximumDecimalNumber {
    let n: id = msg![env; this alloc];
    set_decimal_number_value(env, n, f64::MAX);
    autorelease(env, n)
}

// MARK: - Default behavior

+ (id)defaultBehavior {
    msg_class![env; NSDecimalNumberHandler defaultDecimalNumberHandler]
}

+ (id)notANumber {
    let n: id = msg![env; this alloc];
    set_decimal_number_value(env, n, f64::NAN);
    autorelease(env, n)
}

// MARK: - Constructors

+ (id)decimalNumberWithString:(id)string { // NSString*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithString:string];
    autorelease(env, new)
}

+ (id)decimalNumberWithString:(id)string
                       locale:(id)_locale { // NSString*, id
    msg_class![env; NSDecimalNumber decimalNumberWithString:string]
}

+ (id)decimalNumberWithDecimal:(NSDecimal)decimal {
    // NSDecimal is passed by pointer in the C API but as a value in ObjC.
    // We receive it as a raw bytes blob via the host; treat as f64.
    let new: id = msg![env; this alloc];
    set_decimal_number_value(env, new, decimal as f64);
    autorelease(env, new)
}

+ (id)decimalNumberWithMantissa:(u64)mantissa
                       exponent:(i16)exponent
                     isNegative:(bool)is_negative {
    let new: id = msg![env; this alloc];
    let value = mantissa as f64 * 10f64.powi(exponent as i32)
        * if is_negative { -1.0 } else { 1.0 };
    set_decimal_number_value(env, new, value);
    autorelease(env, new)
}

- (id)initWithString:(id)string { // NSString*
    if string == nil {
        release(env, this);
        return nil;
    }
    let s = to_rust_string(env, string).into_owned();
    let value = s.trim().parse::<f64>().unwrap_or(0.0);
    set_decimal_number_value(env, this, value);
    this
}

- (id)initWithString:(id)string locale:(id)_locale {
    msg![env; this initWithString:string]
}

- (id)initWithDecimal:(NSDecimal)decimal {
    set_decimal_number_value(env, this, decimal as f64);
    this
}

- (id)initWithMantissa:(u64)mantissa
              exponent:(i16)exponent
            isNegative:(bool)is_negative {
    let value = mantissa as f64 * 10f64.powi(exponent as i32)
        * if is_negative { -1.0 } else { 1.0 };
    set_decimal_number_value(env, this, value);
    this
}

// Convenience init from double.
- (id)initWithDouble:(f64)value {
    set_decimal_number_value(env, this, value);
    this
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - NSNumber primitive overrides

- (bool)boolValue {
    decimal_number_value(env, this) != 0.0
}

- (i32)intValue {
    decimal_number_value(env, this) as i32
}

- (NSInteger)integerValue {
    decimal_number_value(env, this) as NSInteger
}

- (i32)longValue {
    decimal_number_value(env, this) as i32
}

- (i64)longLongValue {
    decimal_number_value(env, this) as i64
}

- (u32)unsignedIntValue {
    decimal_number_value(env, this) as u32
}

- (NSUInteger)unsignedIntegerValue {
    decimal_number_value(env, this) as NSUInteger
}

- (u64)unsignedLongLongValue {
    decimal_number_value(env, this) as u64
}

- (f32)floatValue {
    decimal_number_value(env, this) as f32
}

- (f64)doubleValue {
    decimal_number_value(env, this)
}

- (i16)shortValue {
    decimal_number_value(env, this) as i16
}

- (u16)unsignedShortValue {
    decimal_number_value(env, this) as u16
}

- (i8)charValue {
    decimal_number_value(env, this) as i8
}

// MARK: - Arithmetic

- (id)decimalNumberByAdding:(id)other { // NSDecimalNumber*
    let a = decimal_number_value(env, this);
    let b = decimal_number_value(env, other);
    let result = decimal_from_f64(env, a + b);
    autorelease(env, result)
}

- (id)decimalNumberBySubtracting:(id)other {
    let a = decimal_number_value(env, this);
    let b = decimal_number_value(env, other);
    let result = decimal_from_f64(env, a - b);
    autorelease(env, result)
}

- (id)decimalNumberByMultiplyingBy:(id)other {
    let a = decimal_number_value(env, this);
    let b = decimal_number_value(env, other);
    let result = decimal_from_f64(env, a * b);
    autorelease(env, result)
}

- (id)decimalNumberByDividingBy:(id)other {
    let a = decimal_number_value(env, this);
    let b = decimal_number_value(env, other);
    if b == 0.0 {
        log!("NSDecimalNumber: divide by zero");
        return msg_class![env; NSDecimalNumber notANumber];
    }
    let result = decimal_from_f64(env, a / b);
    autorelease(env, result)
}

- (id)decimalNumberByRaisingToPower:(NSUInteger)power {
    let a = decimal_number_value(env, this);
    let result = decimal_from_f64(env, a.powi(power as i32));
    autorelease(env, result)
}

- (id)decimalNumberByMultiplyingByPowerOf10:(i16)power {
    let a = decimal_number_value(env, this);
    let result = decimal_from_f64(env, a * 10f64.powi(power as i32));
    autorelease(env, result)
}

// Arithmetic with behaviour (behaviour controls rounding/error — ignored here).
- (id)decimalNumberByAdding:(id)other withBehavior:(id)_behavior {
    msg![env; this decimalNumberByAdding:other]
}
- (id)decimalNumberBySubtracting:(id)other withBehavior:(id)_behavior {
    msg![env; this decimalNumberBySubtracting:other]
}
- (id)decimalNumberByMultiplyingBy:(id)other withBehavior:(id)_behavior {
    msg![env; this decimalNumberByMultiplyingBy:other]
}
- (id)decimalNumberByDividingBy:(id)other withBehavior:(id)_behavior {
    msg![env; this decimalNumberByDividingBy:other]
}
- (id)decimalNumberByRaisingToPower:(NSUInteger)power withBehavior:(id)_behavior {
    msg![env; this decimalNumberByRaisingToPower:power]
}
- (id)decimalNumberByMultiplyingByPowerOf10:(i16)power withBehavior:(id)_behavior {
    msg![env; this decimalNumberByMultiplyingByPowerOf10:power]
}

// MARK: - Rounding

- (id)decimalNumberByRoundingAccordingToBehavior:(id)_behavior {
    // Round to nearest integer as a safe default.
    let a = decimal_number_value(env, this);
    let result = decimal_from_f64(env, a.round());
    autorelease(env, result)
}

// NSNumber's inherited compare:/isEqual: implementations operate on the
// shared NSNumberHostObject and therefore correctly compare decimal numbers
// with ordinary NSNumbers as well.

// MARK: - Accessors

- (NSDecimal)decimalValue {
    // NSDecimal is a complex C struct; return value bits as f64.
    decimal_number_value(env, this) as NSDecimal
}

- (id)objCType {
    // Type encoding for double.
    ns_string::get_static_str(env, "d")
}

// MARK: - String representation

- (id)stringValue {
    msg![env; this description]
}

- (id)descriptionWithLocale:(id)_locale {
    msg![env; this description]
}

- (id)description {
    let value = decimal_number_value(env, this);
    let s = if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    };
    let ns = from_rust_string(env, s);
    autorelease(env, ns)
}

// MARK: - NSCopying

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

@end

// =========================================================================
// NSDecimalNumberHandler
// =========================================================================

@implementation NSDecimalNumberHandler: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSDecimalNumberHandlerHostObject {
        rounding_mode: NSRoundPlain,
        scale: 38,
        raise_on_exactness: false,
        raise_on_overflow: true,
        raise_on_underflow: true,
        raise_on_divide_by_zero: true,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)defaultDecimalNumberHandler {
    env.objc.alloc_static_object(
        this,
        Box::new(NSDecimalNumberHandlerHostObject {
            rounding_mode: NSRoundPlain,
            scale: 38,
            raise_on_exactness: false,
            raise_on_overflow: true,
            raise_on_underflow: true,
            raise_on_divide_by_zero: true,
        }),
        &mut env.mem,
    )
}

// ИСПРАВЛЕНИЕ: Удалены методы инициализации с 6 аргументами,
// так как они превышают лимит макросов ядра и всё равно не используются.

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

- (NSRoundingMode)roundingMode {
    env.objc.borrow::<NSDecimalNumberHandlerHostObject>(this).rounding_mode
}

- (i16)scale {
    env.objc.borrow::<NSDecimalNumberHandlerHostObject>(this).scale
}

// NSDecimalNumberBehaviors protocol stubs.
// (roundingMode is defined above; not duplicated here.)

- (id)exceptionDuringOperation:(id)_operation
                          error:(NSCalculationError)error
                    leftOperand:(id)_left
                   rightOperand:(id)_right {
    log!(
        "NSDecimalNumberHandler exceptionDuringOperation: error={} — returning notANumber",
        error
    );
    msg_class![env; NSDecimalNumber notANumber]
}

@end

};

// MARK: - Type alias (NSDecimal is an opaque struct — we use f64 as a proxy)
type NSDecimal = f64;

// MARK: - Helpers

fn decimal_from_f64(env: &mut crate::Environment, value: f64) -> crate::objc::id {
    let class = env.objc.get_known_class("NSDecimalNumber", &mut env.mem);

    env.objc.alloc_object(
        class,
        Box::new(NSDecimalNumberHostObject {
            superclass: NSNumberHostObject::Double(value),
        }),
        &mut env.mem,
    )
}
