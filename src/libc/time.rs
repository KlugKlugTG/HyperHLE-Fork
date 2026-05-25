/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `time.h` (C) and `sys/time.h` (POSIX)

use crate::dyld::{export_c_func, FunctionExports};
use crate::libc::clocale::{setlocale, LC_CTYPE};
use crate::libc::errno::set_errno;
use crate::libc::stdio::printf::{isspace, isspace_inner};
use crate::mem::{guest_size_of, ConstPtr, GuestUSize, MutPtr, Ptr, SafeRead};
use crate::Environment;
use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

#[derive(Default)]
pub struct State {
    /// Temporary static storage for the return value of `gmtime` or
    /// `localtime`. The standard allows calls to either to overwrite it.
    gmtime_tmp: Option<MutPtr<tm>>,
    /// Address of the guest `timezone` global (seconds west of UTC).
    pub timezone_ptr: Option<MutPtr<i32>>,
    /// Address of the guest `daylight` global (DST flag).
    pub daylight_ptr: Option<MutPtr<i32>>,
    /// Address of the guest `tzname[0]` C string.
    pub tzname_std_ptr: Option<MutPtr<u8>>,
    /// Address of the guest `tzname[1]` C string.
    pub tzname_dst_ptr: Option<MutPtr<u8>>,
    /// Address of the guest `tzname[2]` array.
    pub tzname_array_ptr: Option<MutPtr<ConstPtr<u8>>>,
    /// Whether tzset() has already been called.
    tzset_done: bool,
}

pub fn get_timezone_ptr(env: &mut Environment) -> MutPtr<i32> {
    *env.libc_state.time.timezone_ptr.get_or_insert_with(|| env.mem.alloc_and_write(0i32))
}
pub fn get_daylight_ptr(env: &mut Environment) -> MutPtr<i32> {
    *env.libc_state.time.daylight_ptr.get_or_insert_with(|| env.mem.alloc_and_write(0i32))
}
pub fn get_tzname_std_ptr(env: &mut Environment) -> MutPtr<u8> {
    *env.libc_state.time.tzname_std_ptr.get_or_insert_with(|| env.mem.alloc_and_write_cstr(b"UTC"))
}
pub fn get_tzname_dst_ptr(env: &mut Environment) -> MutPtr<u8> {
    *env.libc_state.time.tzname_dst_ptr.get_or_insert_with(|| env.mem.alloc_and_write_cstr(b""))
}
pub fn get_tzname_array_ptr(env: &mut Environment) -> MutPtr<ConstPtr<u8>> {
    *env.libc_state.time.tzname_array_ptr.get_or_insert_with(|| {
        let std_ptr: ConstPtr<u8> = get_tzname_std_ptr(env).cast_const();
        let dst_ptr: ConstPtr<u8> = get_tzname_dst_ptr(env).cast_const();
        let arr_ptr: MutPtr<ConstPtr<u8>> = env.mem.alloc(guest_size_of::<ConstPtr<u8>>() * 2).cast();
        env.mem.write(arr_ptr, std_ptr);
        env.mem.write(arr_ptr + 1, dst_ptr);
        arr_ptr
    })
}

fn maybe_tzset(env: &mut Environment) {
    if !env.libc_state.time.tzset_done {
        tzset(env);
    }
}

fn parse_tz(tz: &str) -> Option<(i32, &str, &str)> {
    let bytes = tz.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !bytes[i].is_ascii_digit() && bytes[i] != b'+' && bytes[i] != b'-' {
        i += 1;
    }
    let std_name = std::str::from_utf8(&bytes[..i]).ok()?;
    if i >= bytes.len() {
        return Some((0, std_name, ""));
    }
    let sign_start = i;
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
    }
    let start_h = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let num_str = std::str::from_utf8(&bytes[sign_start..i]).ok()?;
    let hours: i32 = num_str.parse().ok()?;
    let mut offset_secs = hours * 3600;
    if i < bytes.len() && bytes[i] == b':' {
        i += 1;
        let start_m = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let mins: i32 = std::str::from_utf8(&bytes[start_m..i]).ok()?.parse().ok()?;
        offset_secs += mins * 60;
        if i < bytes.len() && bytes[i] == b':' {
            i += 1;
            let start_s = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let secs: i32 = std::str::from_utf8(&bytes[start_s..i]).ok()?.parse().ok()?;
            offset_secs += secs;
        }
    }
    let dst_name = if i < bytes.len() {
        std::str::from_utf8(&bytes[i..]).ok()?
    } else {
        ""
    };
    Some((offset_secs, std_name, dst_name))
}

// time.h (C)

#[allow(non_camel_case_types)]
/// Time in seconds since UNIX epoch (1970-01-01 00:00:00)
pub type time_t = i32;

#[allow(non_camel_case_types)]
type clock_t = u64;

const CLOCKS_PER_SEC: clock_t = 1000000;

fn clock(env: &mut Environment) -> clock_t {
    // ИСПРАВЛЕНИЕ: Возвращаем точное время в микросекундах (а не усекаем до
    // секунд).
    // Это критически важно для игр (Cocos2D и др.), которые считают дельту
    // времени.
    // Иначе delta time = 0.0, что ведет к делению на ноль -> NaN ->
    // отрицательный sleep -> Crash.
    Instant::now().duration_since(env.startup_time).as_micros() as clock_t
}

fn time(env: &mut Environment, out: MutPtr<time_t>) -> time_t {
    // TODO: handle errno properly
    set_errno(env, 0);
    let time64 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let time = time64 as time_t;
    if time64 != time as u64 {
        log_once!("Warning: [time] system clock is beyond Y2K38 and might confuse the app");
    }
    if !out.is_null() {
        env.mem.write(out, time);
    }
    time
}

fn tzset(env: &mut Environment) {
    env.libc_state.time.tzset_done = true;

    let tz = std::env::var("TZ").unwrap_or_default();
    let (offset_secs, std_name, dst_name) = if tz.is_empty() {
        (0i32, "UTC", "")
    } else {
        parse_tz(&tz).unwrap_or((0, "UTC", ""))
    };

    let tz_ptr = get_timezone_ptr(env);
    env.mem.write(tz_ptr, offset_secs);

    let daylight = if !dst_name.is_empty() { 1i32 } else { 0i32 };
    let dl_ptr = get_daylight_ptr(env);
    env.mem.write(dl_ptr, daylight);

    let std_ptr = env.mem.alloc_and_write_cstr(std_name.as_bytes());
    let dst_ptr = env.mem.alloc_and_write_cstr(dst_name.as_bytes());
    env.libc_state.time.tzname_std_ptr = Some(std_ptr);
    env.libc_state.time.tzname_dst_ptr = Some(dst_ptr);

    let arr_ptr = env.libc_state.time.tzname_array_ptr.unwrap_or_else(|| {
        let arr_ptr: MutPtr<ConstPtr<u8>> = env.mem.alloc(guest_size_of::<ConstPtr<u8>>() * 2).cast();
        env.libc_state.time.tzname_array_ptr = Some(arr_ptr);
        arr_ptr
    });
    env.mem.write(arr_ptr, std_ptr.cast_const());
    env.mem.write(arr_ptr + 1, dst_ptr.cast_const());
}

#[allow(non_camel_case_types)]
#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
/// `struct tm`, fields count from 0 unless marked otherwise
pub struct tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i32,
    tm_zone: ConstPtr<u8>,
}
unsafe impl SafeRead for tm {}

impl tm {
    pub fn from(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        // Some sources of `tm` (e.g. ZIP-archive `DateTime` defaults read
        // by [crate::fs::bundle::IpaFile]) hand us year=0 / month=0 when
        // the archive entry has no recorded date. The previous u16/u8
        // arithmetic `year - 1900` / `month - 1` would overflow in
        // release builds, producing a wildly out-of-range `tm_year`
        // that later panicked the `i64` -> `time_t` (`i32`) cast in
        // [calendar_date_to_timestamp] — which crashed the whole app
        // picker on startup.
        //
        // Clamp to representable values so callers always get a sane
        // timestamp back instead of a panic.
        let year = year.max(1970);
        let month = month.clamp(1, 12);
        let day = day.clamp(1, 31);
        let hour = hour.min(23);
        let minute = minute.min(59);
        let second = second.min(60); // POSIX leap seconds
        tm {
            tm_year: i32::from(year) - 1900,
            tm_mon: i32::from(month) - 1,
            tm_mday: day.into(),
            tm_hour: hour.into(),
            tm_min: minute.into(),
            tm_sec: second.into(),
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: Ptr::null(),
        }
    }
}

const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

const CYCLE_YEARS: i32 = 400;
const YEAR_TO_DAY: [i32; CYCLE_YEARS as usize] = calc_year_to_day().0;
const CYCLE_DAYS: i32 = calc_year_to_day().1;

const fn calc_year_to_day() -> ([i32; CYCLE_YEARS as usize], i32) {
    let mut table = [0i32; CYCLE_YEARS as usize];
    let mut day = 0;
    let mut year = 0;

    while year < CYCLE_YEARS {
        table[year as usize] = day;

        day += if is_leap_year(year) { 366 } else { 365 };
        year += 1;
    }

    (table, day)
}

const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_TO_DAY_NONLEAP: [i32; 12] = calc_month_to_day(false);
const MONTH_TO_DAY_LEAP: [i32; 12] = calc_month_to_day(true);

const fn calc_month_to_day(leap_year: bool) -> [i32; 12] {
    let mut table = [0i32; 12];
    let mut day = 0;
    let mut month = 0;

    while month < 12 {
        table[month] = day;

        day += DAYS_IN_MONTH[month] + ((leap_year && month == 1) as i32);
        month += 1;
    }

    table
}

pub fn timestamp_to_calendar_date(timestamp: time_t) -> tm {
    let seconds_since_unix_epoch: i32 = timestamp;

    let days_since_unix_epoch = seconds_since_unix_epoch.div_euclid(DAY_SECONDS);
    let second_in_day = seconds_since_unix_epoch.rem_euclid(DAY_SECONDS);

    const MINUTE_SECONDS: i32 = 60;
    const HOUR_SECONDS: i32 = MINUTE_SECONDS * 60;
    const DAY_SECONDS: i32 = HOUR_SECONDS * 24;

    let tm_sec = second_in_day % MINUTE_SECONDS;
    let tm_min = (second_in_day % HOUR_SECONDS) / MINUTE_SECONDS;
    let tm_hour = second_in_day / HOUR_SECONDS;

    let days_since_y2k = days_since_unix_epoch - 10957;

    let cycles_since_y2k = days_since_y2k.div_euclid(CYCLE_DAYS);
    let day_in_cycle = days_since_y2k.rem_euclid(CYCLE_DAYS);

    let year_in_cycle: i32 = (YEAR_TO_DAY.partition_point(|&day| day <= day_in_cycle) - 1) as _;
    let year = 2000 + cycles_since_y2k * CYCLE_YEARS + year_in_cycle;
    let day_in_year = day_in_cycle - YEAR_TO_DAY[usize::try_from(year_in_cycle).unwrap()];
    let is_leap_year = is_leap_year(year_in_cycle);

    assert!(day_in_year < (365 + is_leap_year as i32));

    let month_to_day = if is_leap_year {
        &MONTH_TO_DAY_LEAP
    } else {
        &MONTH_TO_DAY_NONLEAP
    };

    let month_in_year: i32 = (month_to_day.partition_point(|&day| day <= day_in_year) - 1) as _;
    let day_in_month = day_in_year - month_to_day[usize::try_from(month_in_year).unwrap()];

    assert!(day_in_month < DAYS_IN_MONTH[month_in_year as usize] + is_leap_year as i32);

    let day_of_the_week = (4 + days_since_unix_epoch).rem_euclid(7);

    tm {
        tm_sec,
        tm_min,
        tm_hour,
        tm_mday: day_in_month + 1,
        tm_mon: month_in_year,
        tm_year: year - 1900,
        tm_wday: day_of_the_week,
        tm_yday: day_in_year,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: Ptr::null(),
    }
}

pub fn calendar_date_to_timestamp(tm: tm) -> time_t {
    // ИСПРАВЛЕНИЕ: Нормализация месяца и года по стандарту POSIX.
    // Если игра передает месяц 12 (или больше), мы конвертируем это в Январь следующего года.
    // Если передает отрицательный месяц - откатываем год назад.
    let mut y = tm.tm_year as i64 + 1900;
    let mut m = tm.tm_mon as i64;

    y += m.div_euclid(12);
    m = m.rem_euclid(12);

    let year = y as i32;
    let month = m as usize;

    let mut seconds = 0i64;

    for y in 1970..year {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        seconds += days_in_year * 86400;
    }

    let days_in_months_cumul = if is_leap_year(year) {
        MONTH_TO_DAY_LEAP[month]
    } else {
        MONTH_TO_DAY_NONLEAP[month]
    };

    seconds += days_in_months_cumul as i64 * 86400;

    // Дни, часы, минуты и секунды можно не нормализовать сложной математикой —
    // при конвертации в секунды они сами "перетекают" куда надо, так как
    // умножаются на свои константы.
    seconds += (tm.tm_mday as i64 - 1) * 86400;
    seconds += tm.tm_hour as i64 * 3600;
    seconds += tm.tm_min as i64 * 60;
    seconds += tm.tm_sec as i64;

    if year < 1970 {
        let mut days_before_year = 0i64;

        for y in year..1970 {
            days_before_year += if is_leap_year(y) { 366 } else { 365 };
        }

        seconds -= days_before_year * 86400;
    }

    seconds.try_into().unwrap_or({
        if seconds > 0 {
            time_t::MAX
        } else {
            time_t::MIN
        }
    })
}

fn gmtime_r(env: &mut Environment, timestamp: ConstPtr<time_t>, res: MutPtr<tm>) -> MutPtr<tm> {
    let timestamp = env.mem.read(timestamp);

    let calendar_date = timestamp_to_calendar_date(timestamp);
    env.mem.write(res, calendar_date);
    res
}
fn gmtime(env: &mut Environment, timestamp: ConstPtr<time_t>) -> MutPtr<tm> {
    let tmp = *env
        .libc_state
        .time
        .gmtime_tmp
        .get_or_insert_with(|| env.mem.alloc(guest_size_of::<tm>()).cast());

    gmtime_r(env, timestamp, tmp)
}

fn localtime_r(env: &mut Environment, timestamp: ConstPtr<time_t>, res: MutPtr<tm>) -> MutPtr<tm> {
    maybe_tzset(env);
    let timestamp = env.mem.read(timestamp);
    let tz = env.libc_state.time.timezone_ptr
        .map(|p| env.mem.read(p))
        .unwrap_or(0);
    let local_timestamp = timestamp - tz;
    let mut calendar_date = timestamp_to_calendar_date(local_timestamp);
    calendar_date.tm_gmtoff = -tz;
    calendar_date.tm_zone = env.libc_state.time.tzname_std_ptr
        .map(|p| p.cast_const())
        .unwrap_or(Ptr::null());
    env.mem.write(res, calendar_date);
    res
}
fn localtime(env: &mut Environment, timestamp: ConstPtr<time_t>) -> MutPtr<tm> {
    let tmp = *env
        .libc_state
        .time
        .gmtime_tmp
        .get_or_insert_with(|| env.mem.alloc(guest_size_of::<tm>()).cast());
    localtime_r(env, timestamp, tmp)
}

fn mktime(env: &mut Environment, tm: MutPtr<tm>) -> time_t {
    maybe_tzset(env);
    let tm_value = env.mem.read(tm);
    let tz = env.libc_state.time.timezone_ptr
        .map(|p| env.mem.read(p))
        .unwrap_or(0);
    let utc_timestamp = calendar_date_to_timestamp(tm_value) + tz;
    let mut normalized = timestamp_to_calendar_date(utc_timestamp - tz);
    normalized.tm_gmtoff = -tz;
    normalized.tm_zone = env.libc_state.time.tzname_std_ptr
        .map(|p| p.cast_const())
        .unwrap_or(Ptr::null());
    env.mem.write(tm, normalized);

    log_dbg!("mktime({:?}) => {}", tm_value, utc_timestamp);
    utc_timestamp
}

// sys/time.h (POSIX)

#[allow(non_camel_case_types)]
type suseconds_t = i32;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C, packed)]
pub(super) struct timeval {
    pub(super) tv_sec: time_t,
    pub(super) tv_usec: suseconds_t,
}
unsafe impl SafeRead for timeval {}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct timespec {
    pub tv_sec: time_t,
    pub tv_nsec: i32,
}
unsafe impl SafeRead for timespec {}

#[allow(non_camel_case_types)]
#[repr(C, packed)]
struct timezone {
    tz_minuteswest: i32,
    tz_dsttime: i32,
}
unsafe impl SafeRead for timezone {}

fn gettimeofday(
    env: &mut Environment,
    timeval_ptr: MutPtr<timeval>,
    timezone_ptr: MutPtr<timezone>,
) -> i32 {
    set_errno(env, 0);

    if !timezone_ptr.is_null() {
        env.mem.write(
            timezone_ptr,
            timezone {
                tz_minuteswest: 0,
                tz_dsttime: 0,
            },
        );
    }

    if timeval_ptr.is_null() {
        return 0;
    }

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let time_s_64: u64 = time.as_secs();
    let tv_sec = time_s_64 as time_t;

    if time_s_64 != tv_sec as u64 {
        log_once!("Warning: [gettimeofday] system clock is beyond Y2K38 and might confuse the app");
    }
    let tv_usec: suseconds_t = time.subsec_micros().try_into().unwrap();

    env.mem.write(timeval_ptr, timeval { tv_sec, tv_usec });

    0
}

fn nanosleep(env: &mut Environment, rqtp: ConstPtr<timespec>, _rmtp: MutPtr<timespec>) -> i32 {
    set_errno(env, 0);

    let t = env.mem.read(rqtp);
    // ИСПРАВЛЕНИЕ: Исключаем панику при отрицательном времени.
    // Функция `try_into().unwrap()` скрашилась бы с `TryFromIntError` при
    // отрицательных значениях от плохих игр.
    // Защищаем Rust-составляющую, ограничивая минимальное время нулем.
    let tv_sec = t.tv_sec.max(0) as u64;
    let tv_nsec = t.tv_nsec.max(0) as u64;
    log_dbg!("nanosleep {} {}", tv_sec, tv_nsec);

    let total_sleep = Duration::from_secs(tv_sec) + Duration::from_nanos(tv_nsec);
    env.sleep(total_sleep);

    0
}

fn strptime(
    env: &mut Environment,
    buffer: ConstPtr<u8>,
    format: ConstPtr<u8>,
    time_ptr: MutPtr<tm>,
) -> MutPtr<u8> {
    log_dbg!(
        "strptime({:?}, {:?})",
        env.mem.cstr_at_utf8(buffer),
        env.mem.cstr_at_utf8(format)
    );

    let mut time_val = env.mem.read(time_ptr);

    let mut conversation_failed = false;
    let mut buffer_char_idx = 0;
    let mut format_char_idx = 0;

    loop {
        let c = env.mem.read(format + format_char_idx);
        format_char_idx += 1;

        if c == b'\0' {
            break;
        }
        if c != b'%' {
            let mut cc = env.mem.read(buffer + buffer_char_idx);

            if isspace(env, format + format_char_idx - 1) {
                while isspace_inner(cc) {
                    buffer_char_idx += 1;
                    cc = env.mem.read(buffer + buffer_char_idx);
                }
                continue;
            }
            if c != cc {
                conversation_failed = true;
                break;
            }
            buffer_char_idx += 1;
            continue;
        }

        let specifier = env.mem.read(format + format_char_idx);
        format_char_idx += 1;

        let mut parse_2_digits = |range: Range<i32>| -> Result<i32, ()> {
            let mut num: i32 = 0;

            let mut chars_count = 0;
            while let c @ b'0'..=b'9' = env.mem.read(buffer + buffer_char_idx) {
                if chars_count >= 2 {
                    break;
                }
                num = num * 10 + (c - b'0') as i32;

                buffer_char_idx += 1;
                chars_count += 1;
            }
            if chars_count != 2 {
                Err(())
            } else if !range.contains(&num) {
                // The guest fed us a syntactically valid 2-digit number that
                // is outside the legal range for this field (e.g. hour=99).
                // Real strptime() returns NULL for this. Don't crash the host.
                log!(
                    "Warning: strptime(): value {} out of range {:?} for field; failing conversion.",
                    num,
                    range
                );
                Err(())
            } else {
                Ok(num)
            }
        };

        match specifier {
            b'H' => match parse_2_digits(0..24) {
                Ok(hour) => {
                    time_val.tm_hour = hour;
                }
                Err(_) => {
                    conversation_failed = true;
                    break;
                }
            },
            b'M' => match parse_2_digits(0..60) {
                Ok(minute) => {
                    time_val.tm_min = minute;
                }
                Err(_) => {
                    conversation_failed = true;
                    break;
                }
            },
            b'S' => match parse_2_digits(0..61) {
                Ok(second) => {
                    time_val.tm_sec = second;
                }
                Err(_) => {
                    conversation_failed = true;
                    break;
                }
            },
            _ => {
                // Unsupported format specifier. Real strptime() returns NULL
                // here; do the same instead of taking down the host.
                log!(
                    "Warning: strptime(): unsupported format character '{}' at index {}; failing conversion.",
                    specifier as char,
                    format_char_idx
                );
                conversation_failed = true;
                break;
            }
        }
    }

    env.mem.write(time_ptr, time_val);

    if conversation_failed {
        Ptr::null()
    } else {
        (buffer + buffer_char_idx).cast_mut()
    }
}

fn strftime(
    env: &mut Environment,
    s: MutPtr<u8>,
    max_size: GuestUSize,
    format: ConstPtr<u8>,
    time_ptr: ConstPtr<tm>,
) -> GuestUSize {
    log_dbg!(
        "strftime({:?}, {}, {:?}, {:?})",
        s,
        max_size,
        env.mem.cstr_at_utf8(format),
        time_ptr
    );

    let ctype_locale = setlocale(env, LC_CTYPE, Ptr::null());
    let ctype_locale_byte = env.mem.read(ctype_locale);
    if ctype_locale_byte != b'C' {
        // We currently only model the C locale. Apps that set a different
        // LC_CTYPE will silently get C-locale formatting; that's fine for
        // numeric format specifiers, just log so we notice.
        log!(
            "Warning: strftime(): unexpected LC_CTYPE locale {:?}; treating as C locale.",
            ctype_locale_byte
        );
    }

    let time_val = env.mem.read(time_ptr);

    let mut res = Vec::<u8>::new();
    let mut format_char_idx = 0;

    loop {
        let c = env.mem.read(format + format_char_idx);
        format_char_idx += 1;

        if c == b'\0' {
            break;
        }
        if c != b'%' {
            res.push(c);
            continue;
        }

        let specifier = env.mem.read(format + format_char_idx);
        format_char_idx += 1;

        match specifier {
            b'm' => {
                let month = (time_val.tm_mon + 1).clamp(1, 12);
                let formatted_month = format!("{:02}", month);
                res.extend_from_slice(formatted_month.as_bytes());
            }
            b'W' => {
                let wday = time_val.tm_wday;
                let yday = time_val.tm_yday;
                // Для %W неделя начинается с понедельника.
                // tm_wday: 0 = Вск, 1 = Пнд... Нам нужно 0 = Пнд... 6 = Вск
                let wday_monday_based = (wday + 6) % 7;

                // Честная формула вычисления номера недели (00-53)
                let week = (yday - wday_monday_based + 7) / 7;
                let formatted_week = format!("{:02}", week);
                res.extend_from_slice(formatted_week.as_bytes());
            }
            b'U' => {
                // Аналогично, но неделя начинается с воскресенья (%U)
                let wday = time_val.tm_wday;
                let yday = time_val.tm_yday;
                let week = (yday - wday + 7) / 7;
                let formatted_week = format!("{:02}", week);
                res.extend_from_slice(formatted_week.as_bytes());
            }
            b'w' => {
                // Номер дня недели от 0 (Воскресенье) до 6 (Суббота)
                let wday = time_val.tm_wday;
                let formatted_wday = format!("{}", wday);
                res.extend_from_slice(formatted_wday.as_bytes());
            }
            b'd' => {
                let day = time_val.tm_mday.clamp(1, 31);
                let formatted_day = format!("{:02}", day);
                res.extend_from_slice(formatted_day.as_bytes());
            }
            b'H' => {
                let hour = time_val.tm_hour.clamp(0, 23);
                let formatted_hour = format!("{:02}", hour);
                res.extend_from_slice(formatted_hour.as_bytes());
            }
            b'M' => {
                let minute = time_val.tm_min.clamp(0, 59);
                let formatted_minute = format!("{:02}", minute);
                res.extend_from_slice(formatted_minute.as_bytes());
            }
            b'b' | b'h' => {
                let month = time_val.tm_mon.clamp(0, 11);
                const MONTH_ABBRS: [&[u8]; 12] = [
                    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct",
                    b"Nov", b"Dec",
                ];
                res.extend_from_slice(MONTH_ABBRS[month as usize]);
            }
            b'a' => {
                let wday = time_val.tm_wday.clamp(0, 6);
                const WDAY_ABBRS: [&[u8]; 7] =
                    [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
                res.extend_from_slice(WDAY_ABBRS[wday as usize]);
            }
            b'Y' => {
                let year = time_val.tm_year + 1900;
                let formatted_year = format!("{:04}", year);
                res.extend_from_slice(formatted_year.as_bytes());
            }
            b'y' => {
                let year = (time_val.tm_year + 1900) % 100;
                let formatted_year = format!("{:02}", year);
                res.extend_from_slice(formatted_year.as_bytes());
            }
            b'Z' => {
                let tz_ptr = time_val.tm_zone;
                if tz_ptr.is_null() {
                    // Эмулятор считает время от UNIX_EPOCH без смещения
                    // (tm_gmtoff = 0),
                    // поэтому мы легально находимся в зоне GMT.
                    res.extend_from_slice(b"GMT");
                } else if let Ok(tz_str) = env.mem.cstr_at_utf8(tz_ptr) {
                    // Если указатель есть — честно читаем зону из памяти гостя
                    res.extend_from_slice(tz_str.as_bytes());
                } else {
                    res.extend_from_slice(b"GMT");
                }
            }
            b'S' => {
                let second = time_val.tm_sec.clamp(0, 60);
                let formatted_second = format!("{:02}", second);
                res.extend_from_slice(formatted_second.as_bytes());
            }
            other => {
                // Unsupported format specifier in strftime(). Emit it
                // literally (preceded by the percent, as glibc does for
                // some unsupported specifiers) and continue instead of
                // crashing the host.
                log!(
                    "Warning: strftime(): unsupported format character '{}' at index {}; emitting literally.",
                    other as char,
                    format_char_idx
                );
                res.push(b'%');
                res.push(other);
            }
        }
    }

    if max_size == 0 {
        // glibc strftime() with max_size=0 returns 0 and writes nothing.
        // Avoid the underflow below.
        return 0;
    }

    let middle = if ((max_size - 1) as usize) < res.len() {
        &res[..(max_size - 1) as usize]
    } else {
        &res[..]
    };

    let dest_slice = env.mem.bytes_at_mut(s, max_size);
    for (i, &byte) in middle.iter().chain(b"\0".iter()).enumerate() {
        dest_slice[i] = byte;
    }

    res.len().try_into().unwrap_or(GuestUSize::MAX)
}

fn difftime(_env: &mut Environment, time1: time_t, time0: time_t) -> f64 {
    // Возвращаем разницу в секундах.
    // Приведение к f64 гарантирует, что мы отдаем честный double, как того ждет
    // игра.
    (time1 as f64) - (time0 as f64)
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(clock()),
    export_c_func!(time(_)),
    export_c_func!(tzset()),
    export_c_func!(gmtime_r(_, _)),
    export_c_func!(gmtime(_)),
    export_c_func!(mktime(_)),
    export_c_func!(localtime_r(_, _)),
    export_c_func!(localtime(_)),
    export_c_func!(gettimeofday(_, _)),
    export_c_func!(nanosleep(_, _)),
    export_c_func!(strptime(_, _, _)),
    export_c_func!(strftime(_, _, _, _)),
    export_c_func!(difftime(_, _)),
];
