//! Compile-time and runtime checks for the Apple ARM64 guest data model.
//!
//! This module is intentionally isolated from the active 32-bit runtime. It
//! records the first safe migration seam: AArch64 uses LP64, so pointers and
//! `long` are 64-bit while `int` remains 32-bit.

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    #[repr(C)]
    struct AppleArm64DataModel {
        pointer: u64,
        long_value: i64,
        int_value: i32,
    }

    #[test]
    fn apple_arm64_uses_lp64_widths() {
        assert_eq!(size_of::<u64>(), 8);
        assert_eq!(size_of::<i64>(), 8);
        assert_eq!(size_of::<i32>(), 4);
        assert_eq!(size_of::<usize>(), 8);
        assert_eq!(size_of::<AppleArm64DataModel>(), 24);
        assert_eq!(align_of::<AppleArm64DataModel>(), 8);
    }

    #[test]
    fn a64_instructions_are_little_endian_words() {
        let ret = 0xd65f03c0_u32;
        assert_eq!(ret.to_le_bytes(), [0xc0, 0x03, 0x5f, 0xd6]);
    }
}
