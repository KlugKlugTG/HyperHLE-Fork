use std::collections::BTreeMap;

use crate::mem::{SafeRead, SafeWrite};

pub type Guest64USize = u64;
pub type Guest64Addr = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub base: Guest64Addr,
    pub size: Guest64USize,
}

#[derive(Debug, Default)]
pub struct Mem64 {
    regions: BTreeMap<Guest64Addr, Vec<u8>>,
    allocations: BTreeMap<Guest64Addr, Guest64USize>,
}

impl Mem64 {
    pub fn new() -> Self { Self::default() }

    pub fn map_zeroed(&mut self, base: Guest64Addr, size: Guest64USize) -> Result<(), &'static str> {
        let size_usize = usize::try_from(size).map_err(|_| "64-bit mapping is too large for this host")?;
        let end = base.checked_add(size).ok_or("64-bit mapping overflows")?;
        if size == 0 { return Ok(()); }
        if let Some((&previous_base, previous)) = self.regions.range(..=base).next_back() {
            let previous_end = previous_base.checked_add(previous.len() as u64).ok_or("mapping overflows")?;
            if previous_end > base { return Err("64-bit mapping overlaps an existing mapping"); }
        }
        if self.regions.range(base..).next().is_some_and(|(&next_base, _)| next_base < end) {
            return Err("64-bit mapping overlaps an existing mapping");
        }
        self.regions.insert(base, vec![0; size_usize]);
        Ok(())
    }

    pub fn write_bytes(&mut self, base: Guest64Addr, bytes: &[u8]) -> Result<(), &'static str> {
        let target = self.slice_mut(base, bytes.len())?;
        target.copy_from_slice(bytes);
        Ok(())
    }

    pub fn alloc_zeroed(&mut self, size: Guest64USize) -> Result<Guest64Addr, &'static str> {
        let size = size.max(16).checked_add(15).ok_or("allocation size overflows")? & !15;
        let base = self.allocations.last_key_value().map_or(0x1_0000_0000, |(base, size)| {
            base.checked_add(*size).unwrap_or(0)
        }).max(0x1_0000_0000);
        self.map_zeroed(base, size)?;
        self.allocations.insert(base, size);
        Ok(base)
    }

    fn region_base(&self, addr: Guest64Addr, size: usize) -> Result<Guest64Addr, &'static str> {
        let (&base, bytes) = self.regions.range(..=addr).next_back().ok_or("64-bit memory access is unmapped")?;
        let offset = addr.checked_sub(base).ok_or("64-bit address underflow")?;
        let end = offset.checked_add(size as u64).ok_or("64-bit access overflows")?;
        if end > bytes.len() as u64 { return Err("64-bit memory access is out of bounds"); }
        Ok(base)
    }

    fn slice(&self, addr: Guest64Addr, size: usize) -> Result<&[u8], &'static str> {
        let base = self.region_base(addr, size)?;
        let offset = usize::try_from(addr - base).map_err(|_| "64-bit offset overflows host usize")?;
        Ok(&self.regions[&base][offset..offset + size])
    }

    fn slice_mut(&mut self, addr: Guest64Addr, size: usize) -> Result<&mut [u8], &'static str> {
        let base = self.region_base(addr, size)?;
        let offset = usize::try_from(addr - base).map_err(|_| "64-bit offset overflows host usize")?;
        Ok(&mut self.regions.get_mut(&base).unwrap()[offset..offset + size])
    }

    pub fn read<T: SafeRead + Copy>(&self, addr: Guest64Addr) -> Result<T, &'static str> {
        let size = std::mem::size_of::<T>();
        let source = self.slice(addr, size)?;
        let mut value = std::mem::MaybeUninit::<T>::uninit();
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), value.as_mut_ptr().cast(), size); Ok(value.assume_init()) }
    }

    pub fn write<T: SafeWrite>(&mut self, addr: Guest64Addr, value: T) -> Result<(), &'static str> {
        let size = std::mem::size_of::<T>();
        let target = self.slice_mut(addr, size)?;
        unsafe { std::ptr::copy_nonoverlapping((&value as *const T).cast(), target.as_mut_ptr(), size) }
        Ok(())
    }

    pub fn read_u8(&self, addr: Guest64Addr) -> Result<u8, &'static str> { self.read(addr) }
    pub fn read_u16(&self, addr: Guest64Addr) -> Result<u16, &'static str> { self.read(addr) }
    pub fn read_u32(&self, addr: Guest64Addr) -> Result<u32, &'static str> { self.read(addr) }
    pub fn read_u64(&self, addr: Guest64Addr) -> Result<u64, &'static str> { self.read(addr) }
    pub fn read_u128(&self, addr: Guest64Addr) -> Result<[u64; 2], &'static str> { self.read(addr) }
    pub fn write_u8(&mut self, addr: Guest64Addr, value: u8) -> Result<(), &'static str> { self.write(addr, value) }
    pub fn write_u16(&mut self, addr: Guest64Addr, value: u16) -> Result<(), &'static str> { self.write(addr, value) }
    pub fn write_u32(&mut self, addr: Guest64Addr, value: u32) -> Result<(), &'static str> { self.write(addr, value) }
    pub fn write_u64(&mut self, addr: Guest64Addr, value: u64) -> Result<(), &'static str> { self.write(addr, value) }
    pub fn write_u128(&mut self, addr: Guest64Addr, value: [u64; 2]) -> Result<(), &'static str> { self.write(addr, value) }

    pub fn mapped_regions(&self) -> impl Iterator<Item = Region> + '_ {
        self.regions.iter().map(|(&base, bytes)| Region { base, size: bytes.len() as u64 })
    }
}
