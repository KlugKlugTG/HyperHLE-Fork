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
    /// Pointer to the _timezone guest global (seconds west of UTC).
    pub timezone_ptr: Option<MutPtr<i32>>,
    /// Pointer to the _daylight guest global (non-zero if DST is observed).
    pub daylight_ptr: Option<MutPtr<i32>>,
    /// Pointers to _tzname[0] and _tzname[1] (timezone abbreviation strings).
    pub tzname_ptrs: Option<(MutPtr<MutPtr<u8>>, MutPtr<MutPtr<u8>>)>,
    /// Whether tzset() has already been called.
    tz_initialized: bool,
    /// Seconds west of UTC for standard time (_timezone value).
    tz_std_offset: i32,
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

/// Parse the POSIX `TZ` environment variable value of the form:
///   `std offset [dst [offset]]`
///
/// Returns `(std_name, std_offset_secs_west, dst_name, dst_offset_secs_west)`
/// where offsets are seconds *west* of UTC (matching the POSIX `timezone`
/// global convention).
///
/// If `dst` is present without its own `offset`, the DST offset is assumed to
/// be 1 hour east of standard time (i.e. 3600 seconds less west).
///
/// References:
/// - POSIX.1-2017 "Environment Variables" (§8.3)
/// - Apple `tzset(3)` man page
fn parse_tz_value(tz_str: &str) -> (&str, i32, &str, i32) {
    let s = tz_str.trim();

    // POSIX allows ":TZname" (implementation-defined) — strip the leading
    // colon and try the POSIX rule parsing anyway; on real macOS the colon
    // prefix causes a zoneinfo lookup, which we don't support yet.
    let s = s.strip_prefix(':').unwrap_or(s);

    if s.is_empty() || s.len() < 3 {
        return ("UTC", 0, "", 0);
    }

    // POSIX TZ rule format: std offset [dst [offset] [,start[/time],end[/time]]]
    // We only need std, offset, and optionally dst name and dst offset.
    let std_len = s
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .count()
        .min(s.len());

    if std_len < 3 {
        return ("UTC", 0, "", 0);
    }

    let std_name = &s[..std_len];
    let rest = &s[std_len..];

    if rest.is_empty() {
        return (std_name, 0, "", 0);
    }

    let parse_posix_offset = |input: &str| -> Option<(i32, usize)> {
        let sign = match input.chars().next() {
            Some('+') => -1, // POSIX: + means east of UTC → less seconds west
            Some('-') => 1,  // POSIX: - means west of UTC → more seconds west
            _ => return None,
        };
        let after_sign = &input[1..];
        let mut consumed = 1;

        let mut total = 0i32;
        let hour_str: String = after_sign
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if hour_str.is_empty() {
            return None;
        }
        consumed += hour_str.len();
        let hours: i32 = hour_str.parse().ok()?;
        total += hours * 3600;

        if consumed < after_sign.len() + 1 {
            let remaining = &after_sign[hour_str.len()..];
            if remaining.starts_with(':') {
                consumed += 1;
                let min_str: String = remaining[1..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !min_str.is_empty() {
                    consumed += min_str.len();
                    let minutes: i32 = min_str.parse().ok()?;
                    total += minutes * 60;

                    if consumed - 1 < after_sign.len() + 1 {
                        let after_min = &remaining[1 + min_str.len()..];
                        if after_min.starts_with(':') {
                            consumed += 1;
                            let sec_str: String = after_min[1..]
                                .chars()
                                .take_while(|c| c.is_ascii_digit())
                                .collect();
                            if !sec_str.is_empty() {
                                consumed += sec_str.len();
                                let seconds: i32 = sec_str.parse().ok()?;
                                total += seconds;
                            }
                        }
                    }
                }
            }
        }

        Some((sign * total, consumed))
    };

    if let Some((offset, consumed)) = parse_posix_offset(rest) {
        let after_offset = &rest[consumed..];

        // Check for DST name (3+ alphabetic chars)
        let dst_len = after_offset
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();

        if dst_len < 3 {
            // No DST — just standard time
            return (std_name, offset, "", 0);
        }

        let dst_name = &after_offset[..dst_len];
        let after_dst = &after_offset[dst_len..];

        // Optional DST offset
        let dst_offset = if let Some((o, _)) = parse_posix_offset(after_dst) {
            o
        } else {
            // DST is 1 hour east of standard (3600 seconds less west)
            offset - 3600
        };

        (std_name, offset, dst_name, dst_offset)
    } else {
        // Could not parse offset
        (std_name, 0, "", 0)
    }
}

fn tzset(env: &mut Environment) {
    let time_state = &mut env.libc_state.time;
    if time_state.tz_initialized {
        return;
    }

    let (std_name, std_offset, dst_name, _dst_offset) = {
        // Read TZ environment variable from the guest environment
        let tz_str = env
            .env_vars
            .get(b"TZ".as_slice())
            .copied()
            .filter(|&ptr| !ptr.is_null())
            .and_then(|ptr| {
                env.mem
                    .cstr_at_utf8(ptr.cast_const())
                    .ok()
                    .map(|s| s.to_string())
            });

        match tz_str {
            Some(ref tz) if !tz.is_empty() => parse_tz_value(tz),
            _ => {
                // No TZ set — try to get the host system timezone offset.
                // Compute the difference between localtime and gmtime via libc;
                // this avoids depending on the tm_gmtoff extension which is not
                // universally available across all libc implementations.
                #[cfg(not(target_arch = "wasm32"))]
                let host_offset: i32 = {
                    let now = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default();
                    let now_secs = now.as_secs() as libc::time_t;
                    (unsafe {
                        let mut tm_local: libc::tm = std::mem::zeroed();
                        let mut tm_utc: libc::tm = std::mem::zeroed();
                        if libc::localtime_r(&now_secs, &mut tm_local).is_null()
                            || libc::gmtime_r(&now_secs, &mut tm_utc).is_null()
                        {
                            return 0;
                        }
                        let local_epoch = libc::mktime(&mut tm_local);
                        let utc_epoch = libc::mktime(&mut tm_utc);
                        // timezone = seconds WEST of UTC
                        // If local_epoch > utc_epoch, local is east → negative timezone
                        let diff: i64 = utc_epoch as i64 - local_epoch as i64;
                        if diff > i32::MAX as i64 {
                            i32::MAX
                        } else if diff < i32::MIN as i64 {
                            i32::MIN
                        } else {
                            diff as i32
                        }
                    })
                };
                #[cfg(target_arch = "wasm32")]
                let host_offset: i32 = 0;

                log_dbg!(
                    "tzset: no TZ env var, using host timezone offset {} seconds west of UTC",
                    host_offset
                );
                ("UTC", host_offset, "", 0)
            }
        }
    };

    // Allocate guest memory for standard timezone name
    let std_name_ptr = env.mem.alloc_and_write_cstr(std_name.as_bytes());

    // Update _timezone (seconds west of UTC)
    if let Some(ptr) = time_state.timezone_ptr {
        env.mem.write(ptr, std_offset);
    }

    // Update _daylight (non-zero if DST is ever observed)
    if let Some(ptr) = time_state.daylight_ptr {
        env.mem.write(ptr, if dst_name.is_empty() { 0 } else { 1 });
    }

    // Update _tzname[0] and _tzname[1]
    if let Some((t0, t1)) = time_state.tzname_ptrs {
        env.mem.write(t0, std_name_ptr);
        if dst_name.is_empty() {
            env.mem.write(t1, MutPtr::<u8>::null());
        } else {
            let dst_name_ptr = env.mem.alloc_and_write_cstr(dst_name.as_bytes());
            env.mem.write(t1, dst_name_ptr);
        }
    }

    time_state.tz_std_offset = std_offset;
    time_state.tz_initialized = true;

    log_dbg!(
        "tzset: std=\"{}\" offset={} (sec west of UTC), dst=\"{}\"",
        std_name,
        std_offset,
        dst_name
    );
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
    // POSIX: localtime_r acts as though tzset() has been called.
    tzset(env);

    let utc_timestamp = env.mem.read(timestamp);
    let tz_offset = env.libc_state.time.tz_std_offset; // seconds WEST of UTC

    // Convert UTC timestamp to local time by subtracting the timezone offset.
    // _timezone is seconds WEST of UTC, so local = UTC - timezone.
    let local_timestamp = utc_timestamp.wrapping_sub(tz_offset);

    let mut calendar_date = timestamp_to_calendar_date(local_timestamp);

    // Populate timezone fields.
    // tm_isdst: 0 for standard time, positive for DST.
    // Currently we don't track actual DST rules, so we report standard time.
    calendar_date.tm_isdst = 0;
    // tm_gmtoff: seconds EAST of UTC (BSD extension).
    // _timezone is seconds WEST, so tm_gmtoff = -_timezone.
    calendar_date.tm_gmtoff = -tz_offset;

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
    let tm_value = env.mem.read(tm);
    let res = calendar_date_to_timestamp(tm_value);

    // ИСПРАВЛЕНИЕ: Стандарт C требует, чтобы mktime нормализовал структуру tm
    // и записал её обратно (с правильными днями недели tm_wday, днем в году tm_yday и т.д.).
    // Мы легко это делаем, прогоняя посчитанный timestamp обратно через конвертер.
    let normalized_tm = timestamp_to_calendar_date(res);
    env.mem.write(tm, normalized_tm);

    log_dbg!("mktime({:?}) => {}", tm_value, res);
    res
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
