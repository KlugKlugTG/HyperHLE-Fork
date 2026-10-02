/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Continuous, gradual value mutation of trainer hits — the per-address
//! "memory breaker".
//!
//! A freeze re-asserts one fixed value forever. These mutations instead keep
//! a value *moving*, which is what makes a game react in interesting ways:
//!
//! - `RAMP` drifts the value by a fixed amount per second (coins ticking up,
//!   a timer running down), clamped to the type's range so it never wraps
//!   around into the opposite extreme.
//! - `RANDOM` picks a fresh value inside a bound a couple of times a second.
//! - `CORRUPT` flips random bits of the current value: the classic Cheat
//!   Engine "corrupt memory" effect, but scoped to a single address instead
//!   of the whole address space (see [`crate::corrupt`] for the global
//!   RTCV-style blast).
//! - `OSC` sweeps between zero and a bound and back again.
//! - `GUARD` records the highest value seen and restores it whenever the game
//!   lowers the value — a "never lose a coin or a hit point" guard.
//!
//! Mutations are driven by the trainer tick, so they are rate-limited, scale
//! with real elapsed time, skip addresses that are no longer inside a live
//! allocation, and never fight a freeze on an overlapping range. Every write
//! goes through [`record_trainer_write`], so the activity feed and the value
//! history classifier never read a trainer mutation as in-game behaviour.

use super::bulk::containing_allocation;
use super::classify::number;
use super::{record_trainer_write, Mem, Patch, SearchResult, VType};
use crate::corrupt::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard cap on simultaneously active mutations (UI and tick-cost sanity).
pub(super) const MAX_MUTATIONS: usize = 64;
/// Random values are drawn this many times per second.
const RANDOM_PER_SEC: f64 = 2.0;
/// One full out-and-back sweep of an oscillation takes this long.
const OSC_PERIOD_SECS: f64 = 4.0;
/// Most bit flips per second a corruptor will perform.
const MAX_BITS_PER_SEC: f64 = 64.0;

/// The kinds of continuous mutation the overlay offers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MutateKind {
    Ramp,
    Random,
    Corrupt,
    Oscillate,
    Guard,
}

impl MutateKind {
    /// Cycling order, also used for hack-file keyword lookup.
    pub const ALL: [MutateKind; 5] = [
        MutateKind::Ramp,
        MutateKind::Random,
        MutateKind::Corrupt,
        MutateKind::Oscillate,
        MutateKind::Guard,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MutateKind::Ramp => "RAMP",
            MutateKind::Random => "RANDOM",
            MutateKind::Corrupt => "CORRUPT",
            MutateKind::Oscillate => "OSC",
            MutateKind::Guard => "GUARD",
        }
    }

    /// Hack-file keyword: `0x1234=5 # I32 ramp`.
    pub fn keyword(self) -> &'static str {
        match self {
            MutateKind::Ramp => "ramp",
            MutateKind::Random => "random",
            MutateKind::Corrupt => "corrupt",
            MutateKind::Oscillate => "osc",
            MutateKind::Guard => "guard",
        }
    }

    /// Look a mode up by its hack-file keyword.
    pub fn from_keyword(word: &str) -> Option<MutateKind> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.keyword().eq_ignore_ascii_case(word))
    }
}

/// A configured continuous mutation of one address.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum MutateMode {
    /// Drift by `step` per second. The step may be negative.
    Ramp { step: f64 },
    /// Uniform value in `0..=max`, `per_sec` times per second.
    Random { max: f64, per_sec: f64 },
    /// Flip random bits of the current value, `bits_per_sec` bits per second.
    Corrupt { bits_per_sec: f64 },
    /// Sweep `0..=max` and back, one round trip per `period_secs`.
    Oscillate { max: f64, period_secs: f64 },
    /// Restore the highest value ever seen at this address.
    Guard,
}

impl MutateMode {
    /// Build a mode from the overlay's SET field.
    ///
    /// `RAMP` takes the per-second step (default `1`), `CORRUPT` the flips per
    /// second (default `1`), and `RANDOM`/`OSC` the upper bound (default: the
    /// value currently in memory). `GUARD` takes no parameter. The keypad has
    /// no way to type a range separator, so bounded modes always start at 0.
    pub(super) fn parse(
        kind: MutateKind,
        vtype: VType,
        text: &str,
        current: f64,
    ) -> Result<MutateMode, &'static str> {
        if vtype == VType::Auto {
            return Err("PICK A CONCRETE TYPE");
        }
        let (lo, hi) = type_range(vtype);
        let text = text.trim();
        Ok(match kind {
            MutateKind::Ramp => {
                let step = if text.is_empty() { 1.0 } else { parse_number(text)? };
                if step == 0.0 {
                    return Err("STEP MUST NOT BE ZERO");
                }
                // A step wider than the whole range would just slam the value
                // into a bound on the first tick.
                let span = (hi - lo).max(1.0);
                MutateMode::Ramp { step: step.clamp(-span, span) }
            }
            MutateKind::Random => {
                let max = parse_bound(text, current, hi)?;
                MutateMode::Random { max, per_sec: RANDOM_PER_SEC }
            }
            MutateKind::Corrupt => {
                let rate = if text.is_empty() { 1.0 } else { parse_number(text)? };
                if !(rate > 0.0) {
                    return Err("RATE MUST BE > 0");
                }
                MutateMode::Corrupt { bits_per_sec: rate.min(MAX_BITS_PER_SEC) }
            }
            MutateKind::Oscillate => {
                let max = parse_bound(text, current, hi)?;
                MutateMode::Oscillate { max, period_secs: OSC_PERIOD_SECS }
            }
            MutateKind::Guard => MutateMode::Guard,
        })
    }

    /// Which overlay button (and hack-file keyword) this mode belongs to.
    pub fn kind(&self) -> MutateKind {
        match *self {
            MutateMode::Ramp { .. } => MutateKind::Ramp,
            MutateMode::Random { .. } => MutateKind::Random,
            MutateMode::Corrupt { .. } => MutateKind::Corrupt,
            MutateMode::Oscillate { .. } => MutateKind::Oscillate,
            MutateMode::Guard => MutateKind::Guard,
        }
    }

    /// One-line description for the overlay status line.
    pub fn describe(&self) -> String {
        match *self {
            MutateMode::Ramp { step } => format!("RAMP {:+}/S", step),
            MutateMode::Random { max, .. } => format!("RANDOM 0..{}", max),
            MutateMode::Corrupt { bits_per_sec } => format!("CORRUPT {} BIT/S", bits_per_sec),
            MutateMode::Oscillate { max, .. } => format!("OSC 0..{}", max),
            MutateMode::Guard => "GUARD: NEVER DROPS".to_string(),
        }
    }

    /// The parameter written to a hack file next to the mode keyword, so
    /// `SAVE HACK` round-trips a mutation.
    pub fn hack_value(&self) -> String {
        match *self {
            MutateMode::Ramp { step } => format!("{}", step),
            MutateMode::Random { max, .. } | MutateMode::Oscillate { max, .. } => format!("{}", max),
            MutateMode::Corrupt { bits_per_sec } => format!("{}", bits_per_sec),
            MutateMode::Guard => "0".to_string(),
        }
    }
}

/// Inclusive numeric range a type can hold. `Auto` has none: mutations always
/// run against a concrete type resolved from the selected search result.
fn type_range(vtype: VType) -> (f64, f64) {
    match vtype {
        VType::Auto => (0.0, 0.0),
        VType::U8 => (0.0, u8::MAX as f64),
        VType::I8 => (i8::MIN as f64, i8::MAX as f64),
        VType::U16 => (0.0, u16::MAX as f64),
        VType::I16 => (i16::MIN as f64, i16::MAX as f64),
        VType::U32 => (0.0, u32::MAX as f64),
        VType::I32 => (i32::MIN as f64, i32::MAX as f64),
        VType::F32 => (-f32::MAX as f64, f32::MAX as f64),
    }
}

fn parse_number(text: &str) -> Result<f64, &'static str> {
    // `f64::from_str` also accepts "inf" and "NaN", neither of which is a
    // usable step, rate or bound.
    let value: f64 = text.parse().map_err(|_| "BAD NUMBER: DIGITS ONLY")?;
    value.is_finite().then_some(value).ok_or("BAD NUMBER: NOT FINITE")
}

/// Upper bound for the bounded modes: the typed value, or the current value
/// when nothing was typed, clamped into the type's positive range.
fn parse_bound(text: &str, current: f64, hi: f64) -> Result<f64, &'static str> {
    let raw = if text.is_empty() { current.max(1.0) } else { parse_number(text)? };
    if !(raw > 0.0) {
        return Err("BOUND MUST BE > 0");
    }
    Ok(raw.min(hi.max(1.0)))
}

/// Raw bits for a numeric value, clamped into the type's range. Integer types
/// round to the nearest representable value; floats keep their fraction.
fn bits_for(vtype: VType, value: f64) -> Option<u64> {
    if !value.is_finite() {
        return None;
    }
    let (lo, hi) = type_range(vtype);
    if !(hi > lo) {
        return None; // Auto: no range, no mutation.
    }
    let value = value.clamp(lo, hi);
    let bits = match vtype {
        VType::Auto => return None,
        VType::U8 => value.round() as u8 as u64,
        VType::I8 => value.round() as i8 as u64,
        VType::U16 => value.round() as u16 as u64,
        VType::I16 => value.round() as i16 as u64,
        VType::U32 => value.round() as u32 as u64,
        VType::I32 => value.round() as i32 as u64,
        VType::F32 => (value as f32).to_bits() as u64,
    };
    Some(mask(vtype, bits))
}

/// Drop the bits a type cannot hold (sign extension from a narrow cast).
fn mask(vtype: VType, bits: u64) -> u64 {
    match vtype.size() {
        1 => bits & 0xFF,
        2 => bits & 0xFFFF,
        _ => bits & 0xFFFF_FFFF,
    }
}

/// Per-mutation state that survives between ticks.
#[derive(Copy, Clone, Debug, Default)]
struct Progress {
    /// Fractional step carry (RAMP), rate budget (RANDOM/CORRUPT) or sweep
    /// position in `0..1` (OSC).
    acc: f64,
    /// Highest value seen so far, as raw bits (GUARD).
    peak: u64,
}

/// One active mutation: an address, its type and how it moves.
#[derive(Copy, Clone, Debug)]
pub(super) struct Mutation {
    pub(super) addr: u32,
    pub(super) vtype: VType,
    pub(super) mode: MutateMode,
    progress: Progress,
}

/// Advance one mutation by `dt` seconds, returning the new raw bits or `None`
/// when this tick changes nothing.
///
/// Pure (no memory access), so the whole gradual-change behaviour is testable
/// without a guest. `dt` is clamped to one second: after a long stall the
/// mutation resumes at its normal rate instead of jumping.
fn next_bits(
    mode: &MutateMode,
    vtype: VType,
    current: u64,
    dt: f64,
    progress: &mut Progress,
    rng: &mut Rng,
) -> Option<u64> {
    let dt = dt.clamp(0.0, 1.0);
    let value = number(vtype, current);
    match *mode {
        MutateMode::Ramp { step } => {
            if vtype == VType::F32 {
                return bits_for(vtype, value + step * dt);
            }
            progress.acc += step * dt;
            let whole = progress.acc.trunc();
            if whole == 0.0 {
                return None; // Not a whole unit yet: keep accumulating.
            }
            progress.acc -= whole;
            bits_for(vtype, value + whole)
        }
        MutateMode::Random { max, per_sec } => {
            progress.acc += per_sec.max(0.0) * dt;
            if progress.acc < 1.0 {
                return None;
            }
            // Drop the whole budget rather than carrying it, so a stall
            // cannot turn into a burst of draws.
            progress.acc = 0.0;
            let fraction = rng.next_u64() as f64 / u64::MAX as f64;
            bits_for(vtype, fraction * max.max(0.0))
        }
        MutateMode::Corrupt { bits_per_sec } => {
            progress.acc += bits_per_sec.max(0.0) * dt;
            let flips = progress.acc.trunc();
            if flips < 1.0 {
                return None;
            }
            progress.acc -= flips;
            let width = vtype.size() as u64 * 8;
            let mut bits = mask(vtype, current);
            for _ in 0..(flips as u32).min(8) {
                bits ^= 1u64 << rng.below(width);
            }
            Some(bits)
        }
        MutateMode::Oscillate { max, period_secs } => {
            progress.acc = (progress.acc + dt / period_secs.max(0.05)) % 1.0;
            // Triangle wave: 0 -> max -> 0 across one period.
            let t = if progress.acc < 0.5 {
                progress.acc * 2.0
            } else {
                2.0 - progress.acc * 2.0
            };
            bits_for(vtype, t * max.max(0.0))
        }
        MutateMode::Guard => {
            let peak = number(vtype, progress.peak);
            if value > peak {
                progress.peak = mask(vtype, current);
                None
            } else {
                // Equal (or NaN) means nothing to restore.
                (value < peak).then_some(progress.peak)
            }
        }
    }
}

/// Does a frozen patch cover any byte of `addr..addr+size`?
pub(super) fn overlaps_frozen(frozen: &[Patch], addr: u32, size: u32) -> bool {
    let end = addr as u64 + size as u64;
    frozen
        .iter()
        .any(|p| (p.addr as u64) < end && (addr as u64) < p.addr as u64 + p.vtype.size() as u64)
}

/// Seed for a session's randomness: corruption should not replay identically
/// on every launch, while explicit seeds (tests) stay reproducible.
fn session_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0)
}

/// The set of active mutations, at most one per (address, type) pair.
#[derive(Debug)]
pub(super) struct Mutations {
    entries: Vec<Mutation>,
    rng: Rng,
}

impl Default for Mutations {
    fn default() -> Self {
        Mutations::seeded(session_seed())
    }
}

impl Mutations {
    /// A reproducible set (used by tests and by hack files loaded at start).
    pub(super) fn seeded(seed: u64) -> Mutations {
        Mutations { entries: Vec::new(), rng: Rng::new(seed) }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn mode_of(&self, addr: u32, vtype: VType) -> Option<MutateMode> {
        self.entries
            .iter()
            .find(|m| m.addr == addr && m.vtype == vtype)
            .map(|m| m.mode)
    }

    /// Stop everything; returns how many were running.
    pub(super) fn clear(&mut self) -> usize {
        let count = self.entries.len();
        self.entries.clear();
        count
    }

    /// Start a mutation, replacing any existing one for the same address and
    /// type. The value currently in memory seeds GUARD's peak, so a negative
    /// starting value is not "restored" up to zero.
    pub(super) fn add(&mut self, mem: &Mem, addr: u32, vtype: VType, mode: MutateMode) -> bool {
        let existing = self
            .entries
            .iter()
            .position(|m| m.addr == addr && m.vtype == vtype);
        if existing.is_none() && self.entries.len() >= MAX_MUTATIONS {
            return false;
        }
        let mutation = Mutation {
            addr,
            vtype,
            mode,
            progress: Progress { acc: 0.0, peak: vtype.read_at(mem, addr).unwrap_or(0) },
        };
        match existing {
            Some(index) => self.entries[index] = mutation,
            None => self.entries.push(mutation),
        }
        true
    }

    /// Apply every mutation once. Returns `(writes, dropped)`, where dropped
    /// counts addresses that left live memory and were removed for good.
    pub(super) fn tick(
        &mut self,
        mem: &mut Mem,
        results: &mut [SearchResult],
        frozen: &[Patch],
        dt: f32,
    ) -> (usize, usize) {
        if self.entries.is_empty() {
            return (0, 0);
        }
        let Mutations { entries, rng } = self;
        let mut allocations = mem.live_allocations();
        allocations.sort_unstable_by_key(|allocation| allocation.0);
        let mut writes = 0;
        let mut dropped = 0;
        let dt = dt as f64;
        entries.retain_mut(|mutation| {
            // Only an address that is still inside a live allocation can be
            // read or written; anything else is dropped for good.
            let in_allocation =
                containing_allocation(&allocations, mutation.addr, mutation.vtype.size()).is_some();
            let current = if in_allocation {
                mutation.vtype.read_at(mem, mutation.addr)
            } else {
                None
            };
            let Some(current) = current else {
                dropped += 1;
                return false;
            };
            // A freeze wins: two writers fighting over one address would
            // only make the value jitter.
            if overlaps_frozen(frozen, mutation.addr, mutation.vtype.size()) {
                mutation.progress.peak = mask(mutation.vtype, current);
                return true;
            }
            let next = next_bits(
                &mutation.mode,
                mutation.vtype,
                current,
                dt,
                &mut mutation.progress,
                rng,
            );
            let Some(next) = next.filter(|&next| next != current) else {
                return true;
            };
            if !mutation.vtype.write_at(mem, mutation.addr, next) {
                return true;
            }
            record_trainer_write(mem, results, mutation.addr, mutation.vtype.size());
            writes += 1;
            true
        });
        (writes, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trainer::tests::{memory, result};

    /// One mutation on `addr`, driven manually so tests control time exactly.
    fn one(mem: &Mem, addr: u32, vtype: VType, mode: MutateMode, seed: u64) -> Mutations {
        let mut mutations = Mutations::seeded(seed);
        assert!(mutations.add(mem, addr, vtype, mode));
        mutations
    }

    fn tick_once(
        mutations: &mut Mutations,
        mem: &mut Mem,
        results: &mut [SearchResult],
        dt: f32,
    ) -> usize {
        mutations.tick(mem, results, &[], dt).0
    }

    #[test]
    fn ramp_drifts_gradually_and_clamps_instead_of_wrapping() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::U8, "250")];
        // 0.25s ticks keep the arithmetic binary-exact, so the expected
        // number of writes is not at the mercy of float accumulation.
        let mut up = one(&mem, base, VType::U8, MutateMode::Ramp { step: 2.0 }, 1);
        // +2/s at a quarter second per tick: half a unit per tick.
        tick_once(&mut up, &mut mem, &mut hits, 0.25);
        assert_eq!(VType::U8.read_at(&mem, base), Some(250), "moved before a whole unit");
        tick_once(&mut up, &mut mem, &mut hits, 0.25);
        assert_eq!(VType::U8.read_at(&mem, base), Some(251), "the ramp never moved");
        for _ in 0..40 {
            tick_once(&mut up, &mut mem, &mut hits, 0.25);
        }
        assert_eq!(VType::U8.read_at(&mem, base), Some(255), "the ramp wrapped at the top");
        let mut down = one(&mem, base, VType::U8, MutateMode::Ramp { step: -4.0 }, 1);
        for _ in 0..300 {
            tick_once(&mut down, &mut mem, &mut hits, 0.25);
        }
        assert_eq!(VType::U8.read_at(&mem, base), Some(0), "the ramp wrapped below zero");
    }

    #[test]
    fn a_sub_unit_step_accumulates_through_the_carry() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "100")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Ramp { step: 1.0 }, 1);
        for tick in 1..4 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.25);
            assert_eq!(VType::I32.read_at(&mem, base), Some(100), "tick {tick}");
        }
        // 4 ticks * 0.25s * 1/s = exactly one unit.
        tick_once(&mut mutations, &mut mem, &mut hits, 0.25);
        assert_eq!(VType::I32.read_at(&mem, base), Some(101));
    }

    #[test]
    fn ramp_moves_floats_by_the_fractional_step() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::F32, "1.0")];
        let mut mutations = one(&mem, base, VType::F32, MutateMode::Ramp { step: 0.5 }, 1);
        for _ in 0..4 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.5);
        }
        let value = f32::from_bits(VType::F32.read_at(&mem, base).unwrap() as u32);
        assert!((value - 2.0).abs() < 1e-6, "got {value}");
    }

    #[test]
    fn corrupt_only_flips_bits_inside_the_value() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "0")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Corrupt { bits_per_sec: 20.0 }, 7);
        let mut ever_changed = false;
        for _ in 0..50 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
            let bits = VType::I32.read_at(&mem, base).unwrap();
            assert!(bits <= 0xFFFF_FFFF, "corruption escaped the type width");
            ever_changed |= bits != 0;
        }
        assert!(ever_changed, "corruption never changed the value");
        // The corruption stayed inside the targeted word.
        assert_eq!(snapshot_of(&mem, base + 4, 8), [0xAA; 8]);
    }

    #[test]
    fn random_stays_inside_the_requested_bound() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "0")];
        let mut mutations = one(
            base,
            VType::I32,
            MutateMode::Random { max: 100.0, per_sec: 1000.0 },
            11,
        );
        let mut seen = [false; 101];
        for _ in 0..400 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
            let value = VType::I32.read_at(&mem, base).unwrap() as usize;
            assert!(value <= 100, "random value {value} escaped the bound");
            seen[value] = true;
        }
        assert!(seen.iter().filter(|&&hit| hit).count() > 20, "values did not vary");
    }

    #[test]
    fn oscillate_sweeps_to_the_bound_and_back() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "0")];
        let mut mutations = one(
            base,
            VType::I32,
            MutateMode::Oscillate { max: 100.0, period_secs: 1.0 },
            3,
        );
        let mut peak = 0;
        let mut lowest_after_peak = 100;
        for _ in 0..40 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
            let value = VType::I32.read_at(&mem, base).unwrap();
            peak = peak.max(value);
            if peak == 100 {
                lowest_after_peak = lowest_after_peak.min(value);
            }
        }
        assert_eq!(peak, 100, "oscillation never reached its bound");
        assert!(lowest_after_peak < 10, "oscillation never came back down");
    }

    #[test]
    fn guard_restores_a_value_the_game_lowered() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "50")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Guard, 1);
        tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
        assert_eq!(VType::I32.read_at(&mem, base), Some(50), "guard wrote on the first tick");
        assert!(VType::I32.write_at(&mut mem, base, 12));
        tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
        assert_eq!(VType::I32.read_at(&mem, base), Some(50));
        // A genuine increase raises the peak instead of being reverted.
        assert!(VType::I32.write_at(&mut mem, base, 80));
        tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
        assert_eq!(VType::I32.read_at(&mem, base), Some(80));
        assert!(VType::I32.write_at(&mut mem, base, 79));
        tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
        assert_eq!(VType::I32.read_at(&mem, base), Some(80));
    }

    #[test]
    fn a_negative_starting_value_is_not_guarded_up_to_zero() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I8, "-5")];
        let mut mutations = one(&mem, base, VType::I8, MutateMode::Guard, 1);
        for _ in 0..3 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.05);
        }
        assert_eq!(VType::I8.read_at(&mem, base), Some(0xFB)); // -5
    }

    #[test]
    fn freezes_win_over_mutations() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "10")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Ramp { step: 100.0 }, 1);
        let frozen = [Patch { addr: base, vtype: VType::I32, bits: 10, freeze: true }];
        for _ in 0..20 {
            mutations.tick(&mut mem, &mut hits, &frozen, 0.05);
        }
        assert_eq!(VType::I32.read_at(&mem, base), Some(10));
    }

    #[test]
    fn freed_addresses_are_dropped_not_retried() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "10")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Ramp { step: 100.0 }, 1);
        assert_eq!(mutations.tick(&mut mem, &mut hits, &[], 0.25), (1, 0));
        mem.free(crate::mem::MutVoidPtr::from_bits(base));
        assert_eq!(mutations.tick(&mut mem, &mut hits, &[], 0.25), (0, 1));
        assert!(mutations.is_empty());
    }

    #[test]
    fn mutation_writes_are_not_reported_as_game_changes() {
        let (mut mem, base) = memory(64);
        let mut hits = vec![result(&mut mem, base, VType::I32, "10")];
        let mut mutations = one(&mem, base, VType::I32, MutateMode::Ramp { step: 100.0 }, 1);
        for _ in 0..4 {
            tick_once(&mut mutations, &mut mem, &mut hits, 0.25);
        }
        // +25 per quarter-second tick.
        assert_eq!(VType::I32.read_at(&mem, base), Some(110));
        assert_eq!(hits[0].bits, 110, "stored result was left stale");
        assert!(!hits[0].changed, "a trainer write was flagged as a game change");
    }

    #[test]
    fn adding_the_same_address_twice_replaces_the_mode() {
        let (mut mem, base) = memory(64);
        result(&mut mem, base, VType::I32, "10");
        let mut mutations = Mutations::seeded(1);
        assert!(mutations.add(&mem, base, VType::I32, MutateMode::Ramp { step: 1.0 }));
        assert!(mutations.add(&mem, base, VType::I32, MutateMode::Guard));
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations.mode_of(base, VType::I32), Some(MutateMode::Guard));
        // A different type at the same address is a separate mutation.
        assert!(mutations.add(&mem, base, VType::U8, MutateMode::Guard));
        assert_eq!(mutations.len(), 2);
    }

    #[test]
    fn the_mutation_count_is_bounded() {
        let (mut mem, base) = memory(4 * MAX_MUTATIONS as u32 + 64);
        let mut mutations = Mutations::seeded(1);
        for index in 0..(MAX_MUTATIONS + 8) {
            let addr = base + index as u32 * 4;
            result(&mut mem, addr, VType::I32, "1");
            let added = mutations.add(&mem, addr, VType::I32, MutateMode::Guard);
            assert_eq!(added, index < MAX_MUTATIONS, "index {index}");
        }
        assert_eq!(mutations.len(), MAX_MUTATIONS);
        // Replacing an existing entry still works at the cap.
        assert!(mutations.add(&mem, base, VType::I32, MutateMode::Ramp { step: 1.0 }));
        assert_eq!(mutations.len(), MAX_MUTATIONS);
    }

    #[test]
    fn narrow_types_never_leak_wide_bits() {
        for (vtype, text, max) in [
            (VType::U8, "255", 255.0),
            (VType::I8, "127", 127.0),
            (VType::U16, "65535", 65535.0),
            (VType::I16, "32767", 32767.0),
        ] {
            let bound = parse_bound(text, 1.0, type_range(vtype).1).unwrap();
            assert_eq!(bound, max);
            let bits = bits_for(vtype, max + 1000.0).unwrap();
            assert!(bits <= (1u64 << (vtype.size() * 8)) - 1, "{vtype:?} -> {bits:#x}");
            assert_eq!(number(vtype, bits), max);
        }
        assert_eq!(bits_for(VType::Auto, 1.0), None);
        assert_eq!(bits_for(VType::I32, f64::NAN), None);
        assert_eq!(bits_for(VType::I32, f64::INFINITY), None);
        assert_eq!(bits_for(VType::U8, -3.0), Some(0));
        assert_eq!(bits_for(VType::I8, -3.0), Some(0xFD));
    }

    #[test]
    fn modes_are_parsed_from_the_set_field_and_rejected_when_nonsense() {
        assert_eq!(
            MutateMode::parse(MutateKind::Ramp, VType::I32, "", 0.0).unwrap(),
            MutateMode::Ramp { step: 1.0 }
        );
        assert_eq!(
            MutateMode::parse(MutateKind::Ramp, VType::I32, "-2.5", 0.0).unwrap(),
            MutateMode::Ramp { step: -2.5 }
        );
        assert_eq!(
            MutateMode::parse(MutateKind::Random, VType::U8, "", 40.0).unwrap(),
            MutateMode::Random { max: 40.0, per_sec: RANDOM_PER_SEC }
        );
        assert_eq!(
            MutateMode::parse(MutateKind::Corrupt, VType::I32, "4", 0.0).unwrap(),
            MutateMode::Corrupt { bits_per_sec: 4.0 }
        );
        assert_eq!(
            MutateMode::parse(MutateKind::Guard, VType::I32, "whatever", 0.0).unwrap(),
            MutateMode::Guard
        );
        for (kind, text) in [
            (MutateKind::Ramp, "0"),
            (MutateKind::Ramp, "abc"),
            (MutateKind::Ramp, "inf"),
            (MutateKind::Random, "0"),
            (MutateKind::Random, "-5"),
            (MutateKind::Corrupt, "-1"),
        ] {
            assert!(
                MutateMode::parse(kind, VType::I32, text, 10.0).is_err(),
                "{kind:?} {text:?}"
            );
        }
        assert!(MutateMode::parse(MutateKind::Guard, VType::Auto, "", 0.0).is_err());
        // An oversized step or bound is clamped into the type's range rather
        // than rejected: the keypad cannot type a smaller one on a U8.
        let clamped = MutateMode::parse(MutateKind::Ramp, VType::U8, "99999", 0.0).unwrap();
        assert_eq!(clamped, MutateMode::Ramp { step: 255.0 });
        let bound = MutateMode::parse(MutateKind::Random, VType::U8, "99999", 0.0).unwrap();
        assert_eq!(bound, MutateMode::Random { max: 255.0, per_sec: RANDOM_PER_SEC });
    }

    #[test]
    fn hack_file_keywords_round_trip() {
        for kind in MutateKind::ALL {
            assert_eq!(MutateKind::from_keyword(kind.keyword()), Some(kind));
            assert_eq!(
                MutateKind::from_keyword(&kind.keyword().to_uppercase()),
                Some(kind)
            );
        }
        assert_eq!(MutateKind::from_keyword("freeze"), None);
        let mode = MutateMode::parse(MutateKind::Ramp, VType::I32, "2", 0.0).unwrap();
        let reparsed =
            MutateMode::parse(MutateKind::Ramp, VType::I32, &mode.hack_value(), 0.0).unwrap();
        assert_eq!(reparsed, mode);
        assert_eq!(MutateMode::Guard.hack_value(), "0");
    }

    fn snapshot_of(mem: &Mem, addr: u32, size: u32) -> Vec<u8> {
        mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(addr), size)
            .unwrap()
            .to_vec()
    }
}
