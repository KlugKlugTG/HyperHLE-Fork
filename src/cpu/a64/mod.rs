//! ARM64 CPU integration boundary.
//!
//! The implementation is intentionally separate from the existing A32 CPU so
//! 32-bit compatibility stays on its proven execution path.

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum A64CpuState {
    Normal,
    Svc(u32),
    Error(A64CpuError),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum A64CpuError {
    MemoryError,
    UndefinedInstruction,
    Breakpoint,
}

pub const GENERAL_REGISTER_COUNT: usize = 31;
pub const SIMD_REGISTER_COUNT: usize = 32;
pub const SIMD_REGISTER_WORDS: usize = 2;
pub const STACK_ALIGNMENT: u64 = 16;
