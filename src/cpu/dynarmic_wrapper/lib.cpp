/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#include <cstdint>
#include <exception>
#include <cstdio>

#include "dynarmic/interface/A32/a32.h"
#include "dynarmic/interface/A32/config.h"
#include "dynarmic/interface/A32/coprocessor.h"
#include "dynarmic/interface/exclusive_monitor.h"

namespace touchHLE::cpu {

using VAddr = std::uint32_t;

// Types and functions defined in Rust
extern "C" {
struct touchHLE_Mem;
std::uint8_t touchHLE_cpu_read_u8(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint16_t touchHLE_cpu_read_u16(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint32_t touchHLE_cpu_read_u32(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint64_t touchHLE_cpu_read_u64(touchHLE_Mem *mem, VAddr addr, bool *error);
bool touchHLE_cpu_write_u8(touchHLE_Mem *mem, VAddr addr, std::uint8_t value);
bool touchHLE_cpu_write_u16(touchHLE_Mem *mem, VAddr addr, std::uint16_t value);
bool touchHLE_cpu_write_u32(touchHLE_Mem *mem, VAddr addr, std::uint32_t value);
bool touchHLE_cpu_write_u64(touchHLE_Mem *mem, VAddr addr, std::uint64_t value);
struct touchHLE_DynarmicContext {
  std::array<std::uint32_t, 16> regs;
  std::array<std::uint32_t, 64> extregs;
  std::uint32_t cpsr;
  std::uint32_t fpscr;
};
}

const auto HaltReasonSvc = Dynarmic::HaltReason::UserDefined1;
const auto HaltReasonUndefinedInstruction = Dynarmic::HaltReason::UserDefined2;
const auto HaltReasonBreakpoint = Dynarmic::HaltReason::UserDefined3;

class Environment final : public Dynarmic::A32::UserCallbacks {
public:
  Dynarmic::A32::Jit *cpu = nullptr;
  touchHLE_Mem *mem = nullptr;
  std::uint64_t ticks_remaining;
  uint32_t halting_svc;
  // Start address of the instruction that most recently raised an
  // UndefinedInstruction/Breakpoint exception. Dynarmic's RaiseException()
  // leaves the guest PC pointing *after* the faulting instruction (start +
  // 2 or + 4 depending on whether it was a 16-bit or 32-bit Thumb
  // encoding), so the Rust side cannot reliably recover the fault address or
  // the instruction length from the register file alone. See
  // `touchHLE_DynarmicWrapper_last_exception_pc`.
  std::uint32_t last_exception_pc = 0;

private:
  std::uint8_t MemoryRead8(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u8(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint16_t MemoryRead16(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u16(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint32_t MemoryRead32(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u32(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint64_t MemoryRead64(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u64(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }

  std::optional<std::uint32_t> MemoryReadCode(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u32(mem, vaddr, &error);
    if (error) {
      return std::nullopt;
    } else {
      return value;
    }
  }

  void MemoryWrite8(VAddr vaddr, std::uint8_t value) override {
    if (touchHLE_cpu_write_u8(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite16(VAddr vaddr, std::uint16_t value) override {
    if (touchHLE_cpu_write_u16(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite32(VAddr vaddr, std::uint32_t value) override {
    if (touchHLE_cpu_write_u32(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite64(VAddr vaddr, std::uint64_t value) override {
    if (touchHLE_cpu_write_u64(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }

  bool MemoryWriteExclusive8(VAddr addr, std::uint8_t value,
                             std::uint8_t expected) override {
    if (MemoryRead8(addr) != expected) {
      return false;
    }
    MemoryWrite8(addr, value);
    return true;
  }
  bool MemoryWriteExclusive16(VAddr addr, std::uint16_t value,
                              std::uint16_t expected) override {
    if (MemoryRead16(addr) != expected) {
      return false;
    }
    MemoryWrite16(addr, value);
    return true;
  }
  bool MemoryWriteExclusive32(VAddr addr, std::uint32_t value,
                              std::uint32_t expected) override {
    if (MemoryRead32(addr) != expected) {
      return false;
    }
    MemoryWrite32(addr, value);
    return true;
  }
  bool MemoryWriteExclusive64(VAddr addr, std::uint64_t value,
                              std::uint64_t expected) override {
    if (MemoryRead64(addr) != expected) {
      return false;
    }
    MemoryWrite64(addr, value);
    return true;
  }

  void InterpreterFallback(std::uint32_t, size_t) override {
    abort(); // TODO
  }
  void CallSVC(std::uint32_t svc) override {
    halting_svc = svc;
    cpu->HaltExecution(HaltReasonSvc);
  }
  void ExceptionRaised(VAddr pc, Dynarmic::A32::Exception exception) override {
    // MemoryReadCode returned nullopt
    if (exception == Dynarmic::A32::Exception::NoExecuteFault) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    } else if (exception == Dynarmic::A32::Exception::UndefinedInstruction) {
      last_exception_pc = pc;
      cpu->HaltExecution(HaltReasonUndefinedInstruction);
    } else if (exception == Dynarmic::A32::Exception::Breakpoint) {
      last_exception_pc = pc;
      cpu->HaltExecution(HaltReasonBreakpoint);
    } else if (exception == Dynarmic::A32::Exception::Yield ||
               exception == Dynarmic::A32::Exception::WaitForEvent ||
               exception == Dynarmic::A32::Exception::WaitForInterrupt ||
               exception == Dynarmic::A32::Exception::SendEvent ||
               exception == Dynarmic::A32::Exception::SendEventLocal ||
               exception == Dynarmic::A32::Exception::PreloadData ||
               exception == Dynarmic::A32::Exception::PreloadDataWithIntentToWrite ||
               exception == Dynarmic::A32::Exception::PreloadInstruction ||
               exception == Dynarmic::A32::Exception::UnpredictableInstruction ||
               exception == Dynarmic::A32::Exception::DecodeError) {
      // Hint instructions (WFE/WFI/YIELD/SEV/PLD): on real hardware these
      // are no-ops or low-power hints, and dynarmic has already advanced the
      // PC past the instruction, so just continue execution without halting.
      // Aborting here (as we used to) killed apps like N.O.V.A. 3 with an
      // unexplained SIGABRT when guest code ran these hints in a spin loop.
    } else {
      // UnpredictableInstruction / DecodeError / anything else: treat exactly
      // like an undefined instruction so the Rust-side graceful bypass
      // (fake return to LR / skip instruction) can handle it instead of
      // aborting the whole host process.
      std::fprintf(stderr, "ExceptionRaised: unexpected exception %u at %x\n",
                   unsigned(exception), pc);
      last_exception_pc = pc;
      cpu->HaltExecution(HaltReasonUndefinedInstruction);
    }
  }
  void AddTicks(std::uint64_t ticks) override {
    if (ticks > ticks_remaining) {
      ticks_remaining = 0;
      return;
    }
    ticks_remaining -= ticks;
  }
  std::uint64_t GetTicksRemaining() override { return ticks_remaining; }
};

// Implements the Dynarmic A32 coprocessor interface by ignoring every
// operation (reads return 0, writes are discarded). Registered for all 16
// coprocessor numbers: any MRC/MCR/LDC/STC the decoders route to a
// coprocessor must get an answer. Without a registration, the AArch64
// emitter hits its `ASSERT_FALSE("Should raise coproc exception here")`
// during block compilation and terminates the whole host process
// (SIGABRT seen in Asphalt 8 1.2.0 on a Samsung Galaxy S25).
class ArmDynarmicIgnoredCoprocessor : public Dynarmic::A32::Coprocessor {
  static std::uint64_t Ignore(void *, std::uint32_t, std::uint32_t) {
    return 0;
  }

  static Callback CallbackForIgnoredOperation() {
    return Callback{&Ignore, std::nullopt};
  }

public:
  using CoprocReg = Dynarmic::A32::CoprocReg;

  CallbackOrAccessOneWord CompileSendOneWord(bool, unsigned, CoprocReg,
                                             CoprocReg, unsigned) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileInternalOperation(bool, unsigned, CoprocReg,
                                                   CoprocReg, CoprocReg,
                                                   unsigned) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessTwoWords CompileSendTwoWords(bool, unsigned, CoprocReg) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessOneWord CompileGetOneWord(bool, unsigned, CoprocReg,
                                            CoprocReg, unsigned) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessTwoWords CompileGetTwoWords(bool, unsigned, CoprocReg) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileLoadWords(bool, bool, CoprocReg,
                                           std::optional<std::uint8_t>) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileStoreWords(bool, bool, CoprocReg,
                                            std::optional<std::uint8_t>) override {
    return CallbackForIgnoredOperation();
  }
};

class DynarmicWrapper {
  Environment env;
  std::unique_ptr<Dynarmic::A32::Jit> cpu;
  std::unique_ptr<Dynarmic::ExclusiveMonitor> mon;
  std::array<std::uint8_t *, Dynarmic::A32::UserConfig::NUM_PAGE_TABLE_ENTRIES>
      page_table;

public:
  DynarmicWrapper(void *direct_memory_access_ptr, size_t null_page_count) {
    Dynarmic::A32::UserConfig user_config;
    user_config.callbacks = &env;
    for (size_t cp = 0; cp < 16; ++cp) {
      user_config.coprocessors[cp] =
          std::make_shared<ArmDynarmicIgnoredCoprocessor>();
    }
    mon = std::make_unique<Dynarmic::ExclusiveMonitor>(1);
    user_config.global_monitor = mon.get();
    // PERF: opt into dynarmic's "unsafe" optimizations. Only the
    // floating-point / codegen ones are enabled: the worst they can do is
    // produce slightly different FP edge-case results (NaN payloads, rounding
    // of FRECPE/FRSQRTE estimates, NEON rounding-mode changes being ignored),
    // which no real iPhone OS game depends on. In return the emitted guest
    // code drops per-instruction FPCR/NaN bookkeeping, which is a significant
    // speedup for VFP/NEON-heavy game code on both x86-64 and AArch64 hosts.
    //
    // Unsafe_IgnoreGlobalMonitor is deliberately NOT enabled: guest
    // applications use LDREX/STREX-based atomics across threads, and ignoring
    // the exclusive monitor would risk subtle synchronization breakage.
    user_config.unsafe_optimizations = true;
    user_config.optimizations =
        user_config.optimizations |
        Dynarmic::OptimizationFlag::Unsafe_UnfuseFMA |
        Dynarmic::OptimizationFlag::Unsafe_ReducedErrorFP |
        Dynarmic::OptimizationFlag::Unsafe_InaccurateNaN |
        Dynarmic::OptimizationFlag::Unsafe_IgnoreStandardFPCRValue;
#ifndef NDEBUG
    user_config.check_halt_on_memory_access = true;
#endif
    if (direct_memory_access_ptr) {
      // Allow fast accesses to all pages other than the null page, which will
      // fall back to a memory callback, which will then abort execution.
      // TODO: Eventually we should use dynarmic's true fastmem mode, but that
      // requires using mmap/mprotect/etc on the host OS so we can still catch
      // null pointer accesses.
      page_table.fill((std::uint8_t *)direct_memory_access_ptr);
      // Note that the null page size is also defined in src/mem.rs.
      static_assert(1 << Dynarmic::A32::UserConfig::PAGE_BITS == 0x1000);

      if (null_page_count > page_table.size()) {
        printf("Too many null pages, %zu requested but maximum is %zu.",
               null_page_count, page_table.size());
        abort();
      }
      for (size_t i = 0; i < null_page_count; i++) {
        page_table[i] = nullptr;
      }
      user_config.page_table = &page_table;
      user_config.absolute_offset_page_table = true;
    }
    cpu = std::make_unique<Dynarmic::A32::Jit>(user_config);
    env.cpu = cpu.get();
  }

  const std::uint32_t *regs() const { return &cpu->Regs().front(); }
  std::uint32_t *regs() { return &cpu->Regs().front(); }

  std::uint32_t cpsr() const { return cpu->Cpsr(); }
  void set_cpsr(std::uint32_t cpsr) { cpu->SetCpsr(cpsr); }

  std::uint32_t last_exception_pc() const { return env.last_exception_pc; }

  void invalidate_cache_range(VAddr start, std::uint32_t size) {
    cpu->InvalidateCacheRange(start, size);
  }

  void swap_context(touchHLE_DynarmicContext *context) {
    touchHLE_DynarmicContext tmp = {cpu->Regs(), cpu->ExtRegs(), cpu->Cpsr(),
                                    cpu->Fpscr()};
    cpu->Regs() = context->regs;
    cpu->ExtRegs() = context->extregs;
    cpu->SetCpsr(context->cpsr);
    cpu->SetFpscr(context->fpscr);
    *context = tmp;
  }

  std::int32_t run_or_step(touchHLE_Mem *mem, std::uint64_t *ticks) {
    env.mem = mem;
    Dynarmic::HaltReason hr;
    if (ticks) {
      env.ticks_remaining = *ticks;
      hr = cpu->Run();
    } else {
      hr = cpu->Step();
    }
    std::int32_t res;
    if ((!hr && ticks) || (hr == Dynarmic::HaltReason::Step && !ticks)) {
      res = -1;
    } else if (Dynarmic::Has(hr, Dynarmic::HaltReason::MemoryAbort)) {
      res = -2;
    } else if (Dynarmic::Has(hr, HaltReasonUndefinedInstruction)) {
      res = -3;
    } else if (Dynarmic::Has(hr, HaltReasonBreakpoint)) {
      res = -4;
    } else if (Dynarmic::Has(hr, HaltReasonSvc)) {
      res = std::int32_t(env.halting_svc);
    } else {
      printf("unhandled halt reason %u\n", unsigned(hr));
      abort();
    }
    env.mem = nullptr;
    if (ticks) {
      *ticks = env.ticks_remaining;
    }
    return res;
  }
};

extern "C" {

DynarmicWrapper *touchHLE_DynarmicWrapper_new(void *direct_memory_access_ptr,
                                              size_t null_page_count) {
  // An exception must not unwind into Rust. The likely one is Dynarmic failing
  // to get executable memory (e.g. JIT not enabled on iOS); return null and let
  // the caller report it.
  try {
    return new DynarmicWrapper(direct_memory_access_ptr, null_page_count);
  } catch (const std::exception &e) {
    fprintf(stderr, "Could not create the Dynarmic CPU: %s\n", e.what());
    return nullptr;
  }
}
void touchHLE_DynarmicWrapper_delete(DynarmicWrapper *cpu) { delete cpu; }

const std::uint32_t *
touchHLE_DynarmicWrapper_regs_const(const DynarmicWrapper *cpu) {
  return cpu->regs();
}
std::uint32_t *touchHLE_DynarmicWrapper_regs_mut(DynarmicWrapper *cpu) {
  return cpu->regs();
}

std::uint32_t touchHLE_DynarmicWrapper_cpsr(const DynarmicWrapper *cpu) {
  return cpu->cpsr();
}
void touchHLE_DynarmicWrapper_set_cpsr(DynarmicWrapper *cpu,
                                       std::uint32_t cpsr) {
  cpu->set_cpsr(cpsr);
}

std::uint32_t
touchHLE_DynarmicWrapper_last_exception_pc(const DynarmicWrapper *cpu) {
  return cpu->last_exception_pc();
}

void touchHLE_DynarmicWrapper_swap_context(DynarmicWrapper *cpu,
                                           touchHLE_DynarmicContext *context) {
  cpu->swap_context(context);
}

void touchHLE_DynarmicWrapper_invalidate_cache_range(DynarmicWrapper *cpu,
                                                     VAddr start,
                                                     std::uint32_t size) {
  cpu->invalidate_cache_range(start, size);
}

std::int32_t touchHLE_DynarmicWrapper_run_or_step(DynarmicWrapper *cpu,
                                                  touchHLE_Mem *mem,
                                                  std::uint64_t *ticks) {
  return cpu->run_or_step(mem, ticks);
}
}

} // namespace touchHLE::cpu
