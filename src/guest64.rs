use std::convert::TryFrom;

pub const ADDRESS_BITS: u32 = 64;
pub const POINTER_SIZE: usize = 8;
pub const LONG_SIZE: usize = 8;
pub const INT_SIZE: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct GuestAddr(u64);

impl GuestAddr {
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, offset: u64) -> Option<Self> {
        self.0.checked_add(offset).map(Self)
    }

    pub fn checked_sub(self, offset: u64) -> Option<Self> {
        self.0.checked_sub(offset).map(Self)
    }

    pub fn checked_host_offset(self, memory_size: usize) -> Option<usize> {
        let offset = usize::try_from(self.0).ok()?;
        (offset < memory_size).then_some(offset)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct A64RegisterFile {
    x: [u64; 31],
    sp: u64,
    pc: u64,
    pstate: u32,
}

impl A64RegisterFile {
    pub fn x(&self, index: usize) -> Option<u64> {
        self.x.get(index).copied()
    }

    pub fn set_x(&mut self, index: usize, value: u64) -> bool {
        if let Some(register) = self.x.get_mut(index) {
            *register = value;
            true
        } else {
            false
        }
    }

    pub const fn sp(&self) -> u64 {
        self.sp
    }

    pub const fn pc(&self) -> u64 {
        self.pc
    }

    pub const fn pstate(&self) -> u32 {
        self.pstate
    }

    pub fn set_sp(&mut self, value: u64) {
        self.sp = value;
    }

    pub fn set_pc(&mut self, value: u64) {
        self.pc = value;
    }

    pub fn set_pstate(&mut self, value: u32) {
        self.pstate = value;
    }
}

pub fn read_u64_le(memory: &[u8], address: GuestAddr) -> Option<u64> {
    let offset = address.checked_host_offset(memory.len())?;
    let bytes = memory.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

pub fn write_u64_le(memory: &mut [u8], address: GuestAddr, value: u64) -> bool {
    let Some(offset) = address.checked_host_offset(memory.len()) else {
        return false;
    };
    let Some(bytes) = memory.get_mut(offset..offset.saturating_add(8)) else {
        return false;
    };
    if bytes.len() != 8 {
        return false;
    }
    bytes.copy_from_slice(&value.to_le_bytes());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_addresses_are_checked_before_host_conversion() {
        let address = GuestAddr::from_bits(0x100);
        assert_eq!(address.checked_host_offset(0x200), Some(0x100));
        assert_eq!(address.checked_host_offset(0x100), None);
        assert_eq!(GuestAddr::from_bits(u64::MAX).checked_add(1), None);
    }

    #[test]
    fn a64_registers_keep_64_bit_values() {
        let mut registers = A64RegisterFile::default();
        assert!(registers.set_x(0, u64::MAX));
        registers.set_sp(0xffff_ffff_ffff_f000);
        registers.set_pc(0x0000_0001_2345_6788);
        assert_eq!(registers.x(0), Some(u64::MAX));
        assert_eq!(registers.sp(), 0xffff_ffff_ffff_f000);
        assert_eq!(registers.pc(), 0x0000_0001_2345_6788);
        assert!(!registers.set_x(31, 1));
    }

    #[test]
    fn guest_memory_uses_little_endian_u64_values() {
        let mut memory = [0u8; 16];
        let address = GuestAddr::from_bits(4);
        assert!(write_u64_le(&mut memory, address, 0x1122_3344_5566_7788));
        assert_eq!(&memory[4..12], &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
        assert_eq!(read_u64_le(&memory, address), Some(0x1122_3344_5566_7788));
        assert!(!write_u64_le(&mut memory, GuestAddr::from_bits(9), 1));
    }
}
