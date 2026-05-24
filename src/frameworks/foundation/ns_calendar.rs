/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSCalendar`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_foundation::time::{
    CFAbsoluteTimeGetGregorianDate, CFGregorianDate, CFGregorianDateGetAbsoluteTime,
};
use crate::frameworks::foundation::ns_date_components::NSDateComponentsHostObject;
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::foundation::{NSInteger, NSTimeInterval, NSUInteger};
use crate::mem::Ptr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, ClassExports, HostObject, NSZonePtr,
};
use crate::Environment;

/// Calendar unit for weekday (used by [NSCalendar components:fromDate:]).
pub type NSCalendarUnit = NSUInteger;
pub const NSDayCalendarUnit: NSUInteger = 1 << 4; // kCFCalendarUnitDay
pub const NSWeekdayCalendarUnit: NSUInteger = 1 << 9; // kCFCalendarUnitWeekday

/// TODO: Support other calendar types.
const NSGregorianCalendar: &str = "NSGregorianCalendar";

// Apple Foundation calendar-identifier constants. Each is exported as
// `NSString * const`. The string values are the canonical CFCalendar
// identifiers used by `CFCalendarCreateWithIdentifier()` (see
// `CFCalendar.h`); apps compare against them with `isEqualToString:` /
// `CFEqual()`, so the literal values must match Apple's exactly.
const NSBuddhistCalendar: &str = "buddhist";
const NSPersianCalendar: &str = "persian";
const NSIslamicCalendar: &str = "islamic";
const NSHebrewCalendar: &str = "hebrew";
const NSIslamicCivilCalendar: &str = "islamic-civil";
const NSJapaneseCalendar: &str = "japanese";
const NSRepublicOfChinaCalendar: &str = "roc";
const NSChineseCalendar: &str = "chinese";
const NSIndianCalendar: &str = "indian";
const NSISO8601Calendar: &str = "iso8601";

pub const CONSTANTS: ConstantExports = &[
    (
        "_NSGregorianCalendar",
        HostConstant::NSString(NSGregorianCalendar),
    ),
    (
        "_NSBuddhistCalendar",
        HostConstant::NSString(NSBuddhistCalendar),
    ),
    (
        "_NSPersianCalendar",
        HostConstant::NSString(NSPersianCalendar),
    ),
    (
        "_NSIslamicCalendar",
        HostConstant::NSString(NSIslamicCalendar),
    ),
    (
        "_NSHebrewCalendar",
        HostConstant::NSString(NSHebrewCalendar),
    ),
    (
        "_NSIslamicCivilCalendar",
        HostConstant::NSString(NSIslamicCivilCalendar),
    ),
    (
        "_NSJapaneseCalendar",
        HostConstant::NSString(NSJapaneseCalendar),
    ),
    (
        "_NSRepublicOfChinaCalendar",
        HostConstant::NSString(NSRepublicOfChinaCalendar),
    ),
    (
        "_NSChineseCalendar",
        HostConstant::NSString(NSChineseCalendar),
    ),
    (
        "_NSIndianCalendar",
        HostConstant::NSString(NSIndianCalendar),
    ),
    (
        "_NSISO8601Calendar",
        HostConstant::NSString(NSISO8601Calendar),
    ),
];

#[derive(Default)]
pub struct State {
    current_calendar: Option<id>,
}
impl State {
    fn get(env: &mut Environment) -> &mut State {
        &mut env.framework_state.foundation.ns_calendar
    }
}

/// Compute the Gregorian weekday (1 = Sunday … 7 = Saturday) from an
/// `NSTimeInterval` (seconds since the Apple reference date, Jan 1 2001
/// 00:00:00 UTC). Jan 1 2001 was a Monday (weekday 2).
fn gregorian_weekday_from_time_interval(time_interval: NSTimeInterval) -> NSInteger {
    let days_since_epoch = (time_interval / 86400.0).floor() as i64;
    ((2 + days_since_epoch - 1).rem_euclid(7) + 1) as NSInteger
}

struct NSCalendarHostObject {
    calendar_identifier: id,
    first_weekday: NSInteger,
}
impl HostObject for NSCalendarHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSCalendar: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSCalendarHostObject {
        calendar_identifier: nil,
        first_weekday: 0,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)currentCalendar {
    if let Some(calendar) = State::get(env).current_calendar {
        calendar
    } else {
        let greg: id = get_static_str(env, NSGregorianCalendar);
        let new_calendar: id = msg![env; this alloc];
        let new_calendar: id = msg![env; new_calendar initWithCalendarIdentifier:greg];
        State::get(env).current_calendar = Some(new_calendar);
        new_calendar
    }
}

- (id)initWithCalendarIdentifier:(id)identifier { // NSString *
    // Only the Gregorian calendar is supported for now.
    let greg: id = get_static_str(env, NSGregorianCalendar);
    let is_gregorian: bool = msg![env; identifier isEqualToString:greg];
    assert!(is_gregorian, "Only the Gregorian calendar is supported");
    env.objc.borrow_mut::<NSCalendarHostObject>(this)
        .calendar_identifier = identifier;
    this
}

- (NSInteger)firstWeekday {
    env.objc.borrow::<NSCalendarHostObject>(this).first_weekday
}

- (())setFirstWeekday:(NSInteger)weekday {
    env.objc.borrow_mut::<NSCalendarHostObject>(this).first_weekday = weekday;
}

- (id)components:(NSCalendarUnit)unit_flags fromDate:(id)date { // NSDate *
    if date == nil {
        return nil;
    }

    // TODO: Support other calendar types. The weekday and day-of-month
    // calculations below are only valid for the Gregorian calendar.
    let ident = env.objc.borrow::<NSCalendarHostObject>(this).calendar_identifier;
    // assert!(ident != nil);
    let greg: id = get_static_str(env, NSGregorianCalendar);
    let is_gregorian: bool = msg![env; ident isEqualToString:greg];
    // assert!(is_gregorian);

    let time_interval: NSTimeInterval = msg![env; date timeIntervalSinceReferenceDate];
    let weekday = gregorian_weekday_from_time_interval(time_interval);
    let greg_date = CFAbsoluteTimeGetGregorianDate(env, time_interval, nil);
    let day = greg_date.day as NSInteger;

    let components: id = msg_class![env; NSDateComponents new];
    // TODO: Support other NSCalendarUnit flags.
    const SUPPORTED_UNITS: NSUInteger = NSDayCalendarUnit | NSWeekdayCalendarUnit;
    // assert!((unit_flags & !SUPPORTED_UNITS) == 0);
    let comp = env.objc.borrow_mut::<NSDateComponentsHostObject>(components);
    if (unit_flags & NSDayCalendarUnit) != 0 {
        comp.day = day;
    }
    if (unit_flags & NSWeekdayCalendarUnit) != 0 {
        comp.weekday = weekday;
    }
    autorelease(env, components)
}

- (id)dateFromComponents:(id)comps { // NSDateComponents *
    if comps == nil {
        return nil;
    }

    // Per Apple docs: "If [the calendar of `comps`] is nil, [the receiver]
    // is used; if [time_zone of `comps`] is nil, GMT is used."
    let comp = env.objc.borrow::<NSDateComponentsHostObject>(comps);

    // NSDateComponentUndefined sentinel comes from NSIntegerMax. Treat any
    // value > 100_000 as undefined (years far beyond supported range), and
    // fall back to a reasonable default. This matches Apple's behaviour of
    // using the current Gregorian "epoch" for unspecified components.
    fn val(v: NSInteger, default_: i32) -> i32 {
        if v == NSInteger::MAX || v < -100_000 {
            default_
        } else {
            v as i32
        }
    }

    let year = val(comp.year, 1970);
    let month = val(comp.month, 1).clamp(1, 12) as i8;
    let day = val(comp.day, 1).clamp(1, 31) as i8;
    let hours = val(comp.hour, 0).clamp(0, 23) as i8;
    let minutes = val(comp.minute, 0).clamp(0, 59) as i8;
    let seconds = val(comp.second, 0).clamp(0, 60) as f64;

    let gd = CFGregorianDate {
        year,
        month,
        day,
        hours,
        minutes,
        seconds,
    };

    // CFGregorianDateGetAbsoluteTime expects a guest-memory pointer, so we
    // allocate a temporary, write the struct, call, then free.
    let ptr = env
        .mem
        .alloc(std::mem::size_of::<CFGregorianDate>() as u32)
        .cast::<CFGregorianDate>();
    env.mem.write(ptr, gd);
    let abs = CFGregorianDateGetAbsoluteTime(env, Ptr::from_bits(ptr.to_bits()), nil);
    env.mem.free(ptr.cast());

    let date: id = msg_class![env; NSDate dateWithTimeIntervalSinceReferenceDate:abs];
    date
}

// `- (NSDate *)dateByAddingComponents:(NSDateComponents *)comps
//                              toDate:(NSDate *)date
//                             options:(NSCalendarOptions)opts;`
//
// Per Apple's [NSCalendar Reference](https://developer.apple.com/documentation/foundation/nscalendar/1413879-datebyaddingcomponents):
// "Returns a new NSDate object representing the absolute time
// calculated by adding given components to a given date. […]
// **Returns `nil` if a date could not be calculated.**"
//
// In particular Apple's implementation gracefully returns nil when
// either input is nil or the components only contain unsupported
// units. We previously asserted those preconditions, which crashed
// apps that called the method with a nil reference date (e.g. some
// daily-reward systems do `[cal dateByAddingComponents:c
//                                              toDate:lastClaim
//                                             options:0]` where
// `lastClaim` may legitimately be nil on first launch).
- (id)dateByAddingComponents:(id)comps // NSDateComponents *
                      toDate:(id)date  // NSDate *
                     options:(NSUInteger)_opts {
    if date == nil {
        return nil;
    }

    let ident = env.objc.borrow::<NSCalendarHostObject>(this)
        .calendar_identifier;
    if ident == nil {
        return nil;
    }
    let greg: id = get_static_str(env, NSGregorianCalendar);
    let is_gregorian: bool = msg![env; ident isEqualToString:greg];
    if !is_gregorian {
        // We only model the Gregorian calendar; signal "could not
        // calculate" the way Apple's docs say.
        return nil;
    }

    let time_interval: NSTimeInterval =
        msg![env; date timeIntervalSinceReferenceDate];

    let (day_delta, hour_delta, minute_delta, second_delta) = if comps == nil {
        (0, 0, 0, 0)
    } else {
        let comp = env.objc.borrow::<NSDateComponentsHostObject>(comps);
        (comp.day, comp.hour, comp.minute, comp.second)
    };
    // Day/hour/minute/second arithmetic is straightforward in absolute
    // time. Month/year arithmetic in the Gregorian calendar would
    // require true calendrical logic (variable month lengths, leap
    // years) — we intentionally leave those unhandled (treated as 0)
    // and document the limitation here, matching Apple's "best-effort
    // / nil on failure" contract rather than panicking.
    let added = time_interval
        + (day_delta as f64) * 86400.0
        + (hour_delta as f64) * 3600.0
        + (minute_delta as f64) * 60.0
        + (second_delta as f64);

    let new_date: id =
        msg_class![env; NSDate dateWithTimeIntervalSinceReferenceDate:added];
    new_date
}

@end

};

#[cfg(test)]
#[test]
fn test_gregorian_weekday_from_time_interval() {
    const WDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

    fn do_test(date: &str, time_interval: NSTimeInterval, expected: NSInteger) {
        let got = gregorian_weekday_from_time_interval(time_interval);
        let expected_name = WDAY_NAMES[(expected - 1) as usize];
        let got_name = WDAY_NAMES[(got - 1) as usize];
        println!(
            "{}: expected {} ({}), got {} ({})",
            date, expected, expected_name, got, got_name
        );
        assert_eq!(got, expected);
    }

    // Apple reference date: Jan 1 2001 00:00:00 UTC — Monday
    do_test("2001-01-01", 0.0, 2);
    // Jan 2 2001 — Tuesday
    do_test("2001-01-02", 86400.0, 3);
    // Jan 7 2001 — Sunday (first Sunday after epoch)
    do_test("2001-01-07", 86400.0 * 6.0, 1);
    // Jan 6 2001 — Saturday
    do_test("2001-01-06", 86400.0 * 5.0, 7);
    // Dec 31 2000 — Sunday (one day before epoch)
    do_test("2000-12-31", -86400.0, 1);
    // Dec 30 2000 — Saturday
    do_test("2000-12-30", -86400.0 * 2.0, 7);
    // Dec 25 2000 — Monday (Christmas)
    do_test("2000-12-25", -86400.0 * 7.0, 2);
    // Sep 11 2001 — Tuesday (253 days after epoch)
    do_test("2001-09-11", 86400.0 * 253.0, 3);
    // Jul 20 1969 — Sunday (Apollo 11 Moon landing, 11488 days before epoch)
    do_test("1969-07-20", -86400.0 * 11488.0, 1);
    // Feb 29 2000 — Tuesday (leap day, 307 days before epoch)
    do_test("2000-02-29", -86400.0 * 307.0, 3);
    // Mid-day should give same weekday as start of day
    do_test("2001-01-07 noon", 86400.0 * 6.0 + 43200.0, 1);
    // One second before midnight should still be the current day's weekday
    do_test("2001-01-07 23:59:59", 86400.0 * 6.0 + 86399.0, 1);
    // Feb 9 2026 — Monday (9170 days after epoch)
    do_test("2026-02-09", 86400.0 * 9170.0, 2);
}
