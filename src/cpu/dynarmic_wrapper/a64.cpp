/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <memory>
#include <optional>
#include <cstdlib>

#include "dynarmic/interface/A64/a64.h"
#include "dynarmic/interface/A64/config.h"
#include "dynarmic/interface/exclusive_monitor.h"

namespace touchHLE::cpu {

struct DynarmicWrapper;
using A64Vector = Dynarmic::A64::Vector;
using VAddr = std::uint64_t;

extern "C" {
struct touchHLE_Mem;
std::uint8_t touchHLE_cpu_read_u8_64(touchHLE_Mem*, VAddr, bool*);
std::uint16_t touchHLE_cpu_read_u16_64(touchHLE_Mem*, VAddr, bool*);
std::uint32_t touchHLE_cpu_read_u32_64(touchHLE_Mem*, VAddr, bool*);
std::uint64_t touchHLE_cpu_read_u64_64(touchHLE_Mem*, VAddr, bool*);
std::array<std::uint64_t, 2> touchHLE_cpu_read_u128_64(touchHLE_Mem*, VAddr, bool*);
bool touchHLE_cpu_write_u8_64(touchHLE_Mem*, VAddr, std::uint8_t);
bool touchHLE_cpu_write_u16_64(touchHLE_Mem*, VAddr, std::uint16_t);
bool touchHLE_cpu_write_u32_64(touchHLE_Mem*, VAddr, std::uint32_t);
bool touchHLE_cpu_write_u64_64(touchHLE_Mem*, VAddr, std::uint64_t);
bool touchHLE_cpu_write_u128_64(touchHLE_Mem*, VAddr, std::array<std::uint64_t, 2>);
struct touchHLE_DynarmicA64Context {
  std::array<std::uint64_t, 31> regs;
  std::array<std::array<std::uint64_t, 2>, 32> vectors;
  std::uint64_t sp;
  std::uint64_t pc;
  std::uint32_t pstate;
  std::uint32_t fpcr;
  std::uint32_t fpsr;
};
}

const auto HaltReasonSvc = Dynarmic::HaltReason::UserDefined1;
const auto HaltReasonUndefinedInstruction = Dynarmic::HaltReason::UserDefined2;
const auto HaltReasonBreakpoint = Dynarmic::HaltReason::UserDefined3;

class Environment final : public Dynarmic::A64::UserCallbacks {
public:
  Dynarmic::A64::Jit* cpu = nullptr;
  touchHLE_Mem* mem = nullptr;
  std::uint64_t ticks_remaining = 0;
  std::uint32_t halting_svc = 0;

private:
  template <typename T, typename F>
  T read(VAddr addr, F f) {
    bool error = false;
    T value = f(mem, addr, &error);
    if (error) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    return value;
  }

  std::uint8_t MemoryRead8(VAddr a) override { return read<std::uint8_t>(a, touchHLE_cpu_read_u8_64); }
  std::uint16_t MemoryRead16(VAddr a) override { return read<std::uint16_t>(a, touchHLE_cpu_read_u16_64); }
  std::uint32_t MemoryRead32(VAddr a) override { return read<std::uint32_t>(a, touchHLE_cpu_read_u32_64); }
  std::uint64_t MemoryRead64(VAddr a) override { return read<std::uint64_t>(a, touchHLE_cpu_read_u64_64); }
  A64Vector MemoryRead128(VAddr a) override { return read<A64Vector>(a, touchHLE_cpu_read_u128_64); }

  std::optional<std::uint32_t> MemoryReadCode(VAddr a) override {
    bool error = false;
    auto value = touchHLE_cpu_read_u32_64(mem, a, &error);
    return error ? std::nullopt : std::optional<std::uint32_t>(value);
  }

  void MemoryWrite8(VAddr a, std::uint8_t v) override { if (touchHLE_cpu_write_u8_64(mem, a, v)) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort); }
  void MemoryWrite16(VAddr a, std::uint16_t v) override { if (touchHLE_cpu_write_u16_64(mem, a, v)) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort); }
  void MemoryWrite32(VAddr a, std::uint32_t v) override { if (touchHLE_cpu_write_u32_64(mem, a, v)) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort); }
  void MemoryWrite64(VAddr a, std::uint64_t v) override { if (touchHLE_cpu_write_u64_64(mem, a, v)) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort); }
  void MemoryWrite128(VAddr a, A64Vector v) override { if (touchHLE_cpu_write_u128_64(mem, a, v)) cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort); }

  bool MemoryWriteExclusive8(VAddr a, std::uint8_t v, std::uint8_t e) override { if (MemoryRead8(a) != e) return false; MemoryWrite8(a, v); return true; }
  bool MemoryWriteExclusive16(VAddr a, std::uint16_t v, std::uint16_t e) override { if (MemoryRead16(a) != e) return false; MemoryWrite16(a, v); return true; }
  bool MemoryWriteExclusive32(VAddr a, std::uint32_t v, std::uint32_t e) override { if (MemoryRead32(a) != e) return false; MemoryWrite32(a, v); return true; }
  bool MemoryWriteExclusive64(VAddr a, std::uint64_t v, std::uint64_t e) override { if (MemoryRead64(a) != e) return false; MemoryWrite64(a, v); return true; }
  bool MemoryWriteExclusive128(VAddr a, A64Vector v, A64Vector e) override { if (MemoryRead128(a) != e) return false; MemoryWrite128(a, v); return true; }

  void InterpreterFallback(VAddr pc, size_t count) override {
    std::fprintf(stderr, "A64 interpreter fallback at %llx (%zu)\n", static_cast<unsigned long long>(pc), count);
    cpu->HaltExecution(HaltReasonUndefinedInstruction);
  }
  void CallSVC(std::uint32_t svc) override { halting_svc = svc; cpu->HaltExecution(HaltReasonSvc); }
  void ExceptionRaised(VAddr, Dynarmic::A64::Exception e) override {
    cpu->HaltExecution(e == Dynarmic::A64::Exception::Breakpoint ? HaltReasonBreakpoint : HaltReasonUndefinedInstruction);
  }
  void AddTicks(std::uint64_t n) override { ticks_remaining = n > ticks_remaining ? 0 : ticks_remaining - n; }
  std::uint64_t GetTicksRemaining() override { return ticks_remaining; }
  std::uint64_t GetCNTPCT() override { return 0x10000000000ULL - ticks_remaining; }
};

class A64Wrapper {
  Environment env;
  std::unique_ptr<Dynarmic::A64::Jit> cpu;
  std::unique_ptr<Dynarmic::ExclusiveMonitor> monitor;
public:
  A64Wrapper() {
    Dynarmic::A64::UserConfig config;
    config.callbacks = &env;
    monitor = std::make_unique<Dynarmic::ExclusiveMonitor>(1);
    config.global_monitor = monitor.get();
    cpu = std::make_unique<Dynarmic::A64::Jit>(config);
    env.cpu = cpu.get();
  }
  void swap_context(touchHLE_DynarmicA64Context* c) {
    touchHLE_DynarmicA64Context old{cpu->GetRegisters(), cpu->GetVectors(), cpu->GetSP(), cpu->GetPC(), cpu->GetPstate(), cpu->GetFpcr(), cpu->GetFpsr()};
    cpu->SetRegisters(c->regs);
    cpu->SetVectors(c->vectors);
    cpu->SetSP(c->sp);
    cpu->SetPC(c->pc);
    cpu->SetPstate(c->pstate);
    cpu->SetFpcr(c->fpcr);
    cpu->SetFpsr(c->fpsr);
    *c = old;
  }
  std::int32_t run_or_step(touchHLE_Mem* mem, std::uint64_t* ticks) {
    env.mem = mem;
    Dynarmic::HaltReason reason;
    if (ticks) {
      env.ticks_remaining = *ticks;
      reason = cpu->Run();
    } else {
      reason = cpu->Step();
    }
    std::int32_t result = (!ticks && reason == Dynarmic::HaltReason::Step) || (ticks && !reason) ? -1 : -5;
    if (Dynarmic::Has(reason, Dynarmic::HaltReason::MemoryAbort)) result = -2;
    else if (Dynarmic::Has(reason, HaltReasonUndefinedInstruction)) result = -3;
    else if (Dynarmic::Has(reason, HaltReasonBreakpoint)) result = -4;
    else if (Dynarmic::Has(reason, HaltReasonSvc)) result = static_cast<std::int32_t>(env.halting_svc);
    if (ticks) *ticks = env.ticks_remaining;
    env.mem = nullptr;
    return result;
  }
};

extern "C" {
DynarmicWrapper* touchHLE_DynarmicA64Wrapper_new() { return reinterpret_cast<DynarmicWrapper*>(new A64Wrapper()); }
void touchHLE_DynarmicA64Wrapper_delete(DynarmicWrapper* p) { delete reinterpret_cast<A64Wrapper*>(p); }
void touchHLE_DynarmicA64Wrapper_swap_context(DynarmicWrapper* p, touchHLE_DynarmicA64Context* c) { reinterpret_cast<A64Wrapper*>(p)->swap_context(c); }
std::int32_t touchHLE_DynarmicA64Wrapper_run_or_step(DynarmicWrapper* p, touchHLE_Mem* mem, std::uint64_t* ticks) { return reinterpret_cast<A64Wrapper*>(p)->run_or_step(mem, ticks); }
}
}
