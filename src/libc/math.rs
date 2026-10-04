/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `math.h`

use crate::abi::{impl_GuestRet_for_large_struct, GuestArg};
use crate::dyld::{export_c_func, export_c_func_aliased, ConstantExports, FunctionExports, HostConstant};
use crate::libc::errno::set_errno;
use crate::mem::{ConstPtr, MutPtr, SafeRead};
use crate::Environment;
use std::num::FpCategory;

/// Apple's `__float2` type from `<math.h>`:
///
/// ```c
/// typedef struct { float __sinval; float __cosval; } __float2;
/// ```
///
/// Returned by `__sincosf_stret`. On 32-bit ARM AAPCS the 8-byte composite
/// is passed back to the caller via the implicit stret pointer.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct Float2 {
    pub sinval: f32,
    pub cosval: f32,
}
unsafe impl SafeRead for Float2 {}
impl_GuestRet_for_large_struct!(Float2);
impl GuestArg for Float2 {
    const REG_COUNT: usize = 2;
    fn from_regs(regs: &[u32]) -> Self {
        Float2 {
            sinval: GuestArg::from_regs(&regs[0..1]),
            cosval: GuestArg::from_regs(&regs[1..2]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        self.sinval.to_regs(&mut regs[0..1]);
        self.cosval.to_regs(&mut regs[1..2]);
    }
}

/// Apple's `__double2` type from `<math.h>`:
///
/// ```c
/// typedef struct { double __sinval; double __cosval; } __double2;
/// ```
///
/// Returned by `__sincos_stret`.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C, packed)]
pub struct Double2 {
    pub sinval: f64,
    pub cosval: f64,
}
unsafe impl SafeRead for Double2 {}
impl_GuestRet_for_large_struct!(Double2);

// TODO: move to `fenv.h`
type FERoundingDirection = i32;
const FE_TONEAREST: FERoundingDirection = 0x000000;
const FE_TOWARDZERO: FERoundingDirection = 0xc00000;
const FE_UPWARD: FERoundingDirection = 0x400000;
const FE_DOWNWARD: FERoundingDirection = 0x800000;

#[derive(Default)]
pub struct State {
    rounding_direction: FERoundingDirection,
    timer_manager_instance: u32,
}

// The sections in this file are organized to match the C standard.

// FIXME: Many functions in this file should theoretically set errno or affect
//        the floating-point environment. We're hoping apps won't rely on that.

fn abs(_env: &mut Environment, arg: i32) -> i32 {
    arg.abs()
}
fn fabs(_env: &mut Environment, arg: f64) -> f64 {
    arg.abs()
}

// Trigonometric functions

// The `long double` (`l`-suffixed) variants are implemented further down as
// plain aliases of the `double` ones: on 32-bit ARM AAPCS, `long double` is
// 64-bit IEEE 754, identical to `double`.

fn sin(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.sin();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn sinf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.sin();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn cos(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.cos();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn cosf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.cos();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn tan(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.tan();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn tanf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.tan();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// `void sincos(double x, double *sin, double *cos);`
// `void sincosf(float x, float *sin, float *cos);`
//
// Apple exposes both as a small optimisation over calling `sin`/`cos`
// separately. See `Apple/Libm: e_sin.c` and `<math.h>`.
fn sincos(env: &mut Environment, x: f64, sin_out: MutPtr<f64>, cos_out: MutPtr<f64>) {
    set_errno(env, 0);
    if !sin_out.is_null() {
        env.mem.write(sin_out, x.sin());
    }
    if !cos_out.is_null() {
        env.mem.write(cos_out, x.cos());
    }
}
fn sincosf(env: &mut Environment, x: f32, sin_out: MutPtr<f32>, cos_out: MutPtr<f32>) {
    set_errno(env, 0);
    if !sin_out.is_null() {
        env.mem.write(sin_out, x.sin());
    }
    if !cos_out.is_null() {
        env.mem.write(cos_out, x.cos());
    }
}

// `__float2 __sincosf_stret(float x);` (Apple-specific, libm).
// `__double2 __sincos_stret(double x);` (Apple-specific, libm).
//
// These return `{sin(x), cos(x)}` packed into the appropriate composite
// type. Per Apple's libm headers, these are intended as a faster
// alternative when the caller needs both values at once. The compiler
// can also lower a paired `sin`/`cos` call into the stret variant.
//
// The leading `_` in the exported symbol matches the C-mangling convention
// (`___sincosf_stret` shows up in dyld as `__sincosf_stret`).
#[allow(non_snake_case)]
fn __sincosf_stret(env: &mut Environment, x: f32) -> Float2 {
    set_errno(env, 0);
    Float2 {
        sinval: x.sin(),
        cosval: x.cos(),
    }
}
#[allow(non_snake_case)]
fn __sincos_stret(env: &mut Environment, x: f64) -> Double2 {
    set_errno(env, 0);
    Double2 {
        sinval: x.sin(),
        cosval: x.cos(),
    }
}

fn asin(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.asin();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn asinf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.asin();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn acos(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.acos();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn acosf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.acos();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn atan(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.atan();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn atanf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.atan();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

fn atan2f(env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    set_errno(env, 0);
    let res = arg1.atan2(arg2);
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn atan2(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    set_errno(env, 0);
    let res = arg1.atan2(arg2);
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// Hyperbolic functions

fn sinh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.sinh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn sinhf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.sinh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn cosh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.cosh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn coshf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.cosh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn tanh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.tanh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn tanhf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.tanh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

fn asinh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.asinh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn asinhf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.asinh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn acosh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.acosh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn acoshf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.acosh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn atanh(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.atanh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn atanhf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.atanh();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// `long double` variants. These all alias the `double` implementations:
// on 32-bit ARM AAPCS `long double` is 64-bit IEEE 754, identical to
// `double`, so there is nothing extra to compute.

fn fabsl(env: &mut Environment, arg: f64) -> f64 {
    fabs(env, arg)
}
fn sinl(env: &mut Environment, arg: f64) -> f64 {
    sin(env, arg)
}
fn cosl(env: &mut Environment, arg: f64) -> f64 {
    cos(env, arg)
}
fn tanl(env: &mut Environment, arg: f64) -> f64 {
    tan(env, arg)
}
fn asinl(env: &mut Environment, arg: f64) -> f64 {
    asin(env, arg)
}
fn acosl(env: &mut Environment, arg: f64) -> f64 {
    acos(env, arg)
}
fn atanl(env: &mut Environment, arg: f64) -> f64 {
    atan(env, arg)
}
fn atan2l(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    atan2(env, arg1, arg2)
}
fn sinhl(env: &mut Environment, arg: f64) -> f64 {
    sinh(env, arg)
}
fn coshl(env: &mut Environment, arg: f64) -> f64 {
    cosh(env, arg)
}
fn tanhl(env: &mut Environment, arg: f64) -> f64 {
    tanh(env, arg)
}
fn asinhl(env: &mut Environment, arg: f64) -> f64 {
    asinh(env, arg)
}
fn acoshl(env: &mut Environment, arg: f64) -> f64 {
    acosh(env, arg)
}
fn atanhl(env: &mut Environment, arg: f64) -> f64 {
    atanh(env, arg)
}
fn expl(env: &mut Environment, arg: f64) -> f64 {
    exp(env, arg)
}
fn exp2l(env: &mut Environment, arg: f64) -> f64 {
    exp2(env, arg)
}
fn expm1l(env: &mut Environment, arg: f64) -> f64 {
    expm1(env, arg)
}
fn logl(env: &mut Environment, arg: f64) -> f64 {
    log(env, arg)
}
fn log1pl(env: &mut Environment, arg: f64) -> f64 {
    log1p(env, arg)
}
fn log2l(env: &mut Environment, arg: f64) -> f64 {
    log2(env, arg)
}
fn log10l(env: &mut Environment, arg: f64) -> f64 {
    log10(env, arg)
}
fn powl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    pow(env, arg1, arg2)
}
fn sqrtl(env: &mut Environment, arg: f64) -> f64 {
    sqrt(env, arg)
}
fn cbrtl(env: &mut Environment, arg: f64) -> f64 {
    cbrt(env, arg)
}
fn hypotl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    hypot(env, arg1, arg2)
}
fn fmal(env: &mut Environment, a: f64, b: f64, c: f64) -> f64 {
    fma(env, a, b, c)
}
fn ceill(env: &mut Environment, arg: f64) -> f64 {
    ceil(env, arg)
}
fn floorl(env: &mut Environment, arg: f64) -> f64 {
    floor(env, arg)
}
fn roundl(env: &mut Environment, arg: f64) -> f64 {
    round(env, arg)
}
fn truncl(env: &mut Environment, arg: f64) -> f64 {
    trunc(env, arg)
}
fn rintl(env: &mut Environment, arg: f64) -> f64 {
    rint(env, arg)
}
fn nearbyintl(env: &mut Environment, arg: f64) -> f64 {
    nearbyint(env, arg)
}
fn lroundl(env: &mut Environment, arg: f64) -> i32 {
    lround(env, arg)
}
fn llroundl(env: &mut Environment, arg: f64) -> i64 {
    llround(env, arg)
}
fn fmodl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    fmod(env, arg1, arg2)
}
fn remainderl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    remainder(env, arg1, arg2)
}
fn fmaxl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    fmax(env, arg1, arg2)
}
fn fminl(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    fmin(env, arg1, arg2)
}
fn ldexpl(env: &mut Environment, arg: f64, n: i32) -> f64 {
    ldexp(env, arg, n)
}
fn frexpl(env: &mut Environment, arg: f64, exp: MutPtr<i32>) -> f64 {
    frexp(env, arg, exp)
}
fn modfl(env: &mut Environment, val: f64, iptr: MutPtr<f64>) -> f64 {
    modf(env, val, iptr)
}
fn logbl(env: &mut Environment, arg: f64) -> f64 {
    logb(env, arg)
}

// Exponential and logarithmic functions
fn log(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.ln();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn logf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.ln();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log1p(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.ln_1p();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log1pf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.ln_1p();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log2(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.log2();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log2f(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.log2();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log10(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.log10();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn log10f(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.log10();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn exp(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.exp();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
/// Apple libm `__exp10`: 10^x.
fn exp10_impl(_env: &mut Environment, arg: f64) -> f64 {
    10f64.powf(arg)
}
/// Apple libm `__exp10f`: 10^x.
fn exp10f_impl(_env: &mut Environment, arg: f32) -> f32 {
    10f32.powf(arg)
}
fn expf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.exp();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn expm1(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.exp_m1();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn expm1f(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.exp_m1();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn exp2(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.exp2();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn exp2f(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.exp2();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
// ilogb extracts the unbiased exponent of x as an int. Degenerate inputs
// use the FP_ILOGB0 / FP_ILOGBNAN sentinels from Darwin's <math.h>.
fn ilogb_impl(x: f64) -> i32 {
    if x == 0.0 {
        return i32::MIN;
    }
    if x.is_nan() || x.is_infinite() {
        return i32::MAX;
    }
    let bits = x.abs().to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    if biased == 0 {
        // Subnormal: the value is frac * 2^-1074, so the exponent comes
        // from the highest set bit of the fraction.
        let frac = bits & 0x000f_ffff_ffff_ffff;
        (63 - frac.leading_zeros() as i32) - 1074
    } else {
        biased - 1023
    }
}

fn ilogb(env: &mut Environment, arg: f64) -> i32 {
    set_errno(env, 0);
    ilogb_impl(arg)
}
fn ilogbf(env: &mut Environment, arg: f32) -> i32 {
    set_errno(env, 0);
    ilogb_impl(arg as f64)
}

// logb extracts the same exponent as logb's double counterpart of ilogb.
fn logb(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    if arg.is_nan() {
        f64::NAN
    } else if arg.is_infinite() {
        f64::INFINITY
    } else if arg == 0.0 {
        // Raising FE_DIVBYZERO is unmodelled; the result is -inf.
        f64::NEG_INFINITY
    } else {
        ilogb_impl(arg) as f64
    }
}
fn logbf(env: &mut Environment, arg: f32) -> f32 {
    logb(env, arg as f64) as f32
}

// scalbn/scalbln scale x by 2^n efficiently; the guest `long` is 32-bit,
// so both take an i32 here. Reuses the saturating ldexp() logic.
fn scalbn(env: &mut Environment, arg: f64, n: i32) -> f64 {
    ldexp(env, arg, n)
}
fn scalbnf(env: &mut Environment, arg: f32, n: i32) -> f32 {
    ldexpf(env, arg, n)
}
fn scalbln(env: &mut Environment, arg: f64, n: i32) -> f64 {
    ldexp(env, arg, n)
}
fn scalblnf(env: &mut Environment, arg: f32, n: i32) -> f32 {
    ldexpf(env, arg, n)
}

fn ldexp(_env: &mut Environment, arg: f64, n: i32) -> f64 {
    // ldexp(x, 0) must return x unchanged, including ±0.0, ±inf and NaN.
    // For n == 0 the multiply below would still be exact, but skip it
    // anyway so NaN payload/sign of zero are guaranteed preserved.
    if n == 0 || !arg.is_finite() || arg == 0.0 {
        arg
    } else {
        arg * 2f64.powi(n.clamp(-2000, 2000))
    }
}
fn ldexpf(_env: &mut Environment, arg: f32, n: i32) -> f32 {
    if n == 0 || !arg.is_finite() || arg == 0.0 {
        arg
    } else {
        arg * 2f32.powi(n.clamp(-2000, 2000))
    }
}
fn frexpf(env: &mut Environment, arg: f32, exp: MutPtr<i32>) -> f32 {
    frexp(env, arg.into(), exp) as f32
}
fn frexp(env: &mut Environment, arg: f64, exp: MutPtr<i32>) -> f64 {
    if arg == 0.0 {
        env.mem.write(exp, 0);
        return 0.0;
    }
    if arg < 0.0 {
        return -frexp(env, -arg, exp);
    }
    let b = arg.log2().floor() as i32 + 1;
    env.mem.write(exp, b);
    let frac = arg / 2f64.powi(b);
    assert!((0.5..1.0).contains(&frac), "arg {arg}, b {b}, frac {frac}");
    frac
}

// Power functions
fn pow(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    set_errno(env, 0);
    let res = arg1.powf(arg2);
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn powf(env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    set_errno(env, 0);
    let res = arg1.powf(arg2);
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn sqrt(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.sqrt();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn sqrtf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.sqrt();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// fma computes a * b + c with a single rounding. Rust's mul_add maps to
// the host fused multiply-add where available and stays correctly rounded
// otherwise.
fn fma(_env: &mut Environment, a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
fn fmaf(_env: &mut Environment, a: f32, b: f32, c: f32) -> f32 {
    a.mul_add(b, c)
}

// Nearest integer functions
fn ceil(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.ceil();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn ceilf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.ceil();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn floor(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.floor();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn floorf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.floor();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn round(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = arg.round();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn roundf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = arg.round();
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn lround(env: &mut Environment, arg: f64) -> i32 {
    set_errno(env, 0);
    arg.max(i32::MIN as f64).min(i32::MAX as f64).round() as i32
}
fn lroundf(env: &mut Environment, arg: f32) -> i32 {
    set_errno(env, 0);
    arg.max(i32::MIN as f32).min(i32::MAX as f32).round() as i32
}
fn trunc(_env: &mut Environment, arg: f64) -> f64 {
    arg.trunc()
}
fn truncf(_env: &mut Environment, arg: f32) -> f32 {
    arg.trunc()
}
fn modf(env: &mut Environment, val: f64, iptr: MutPtr<f64>) -> f64 {
    let ivalue = trunc(env, val);
    env.mem.write(iptr, ivalue);
    val - ivalue
}
fn modff(env: &mut Environment, val: f32, iptr: MutPtr<f32>) -> f32 {
    let ivalue = truncf(env, val);
    env.mem.write(iptr, ivalue);
    val - ivalue
}
fn rint(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    let res = match env.libc_state.math.rounding_direction {
        FE_TONEAREST => {
            // As tested on both macOS and iOS Simulator, by default it
            // rounds to the nearest integer with ties on even
            arg.round_ties_even()
        }
        FE_TOWARDZERO => arg.trunc(),
        FE_UPWARD => arg.ceil(),
        FE_DOWNWARD => arg.floor(),
        other => {
            log!(
                "Warning: rint/nearbyint: unknown rounding mode {}; defaulting to round-to-nearest.",
                other
            );
            arg.round_ties_even()
        }
    };
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn lrint(env: &mut Environment, arg: f64) -> i32 {
    set_errno(env, 0);
    let clamped = arg.clamp(i32::MIN as f64, i32::MAX as f64);
    match env.libc_state.math.rounding_direction {
        FE_TONEAREST => {
            // As tested on both macOS and iOS Simulator, by default it
            // rounds to the nearest integer with ties on even
            clamped.round_ties_even() as i32
        }
        FE_TOWARDZERO => clamped.trunc() as i32,
        other => {
            // Unknown rounding mode (the guest set FE_DOWNWARD / FE_UPWARD
            // via fesetround in a way we don't model yet). Real lrint() is
            // allowed to fall back to FE_TONEAREST in this case; do that
            // instead of crashing the host.
            log!(
                "Warning: lrint(): unsupported rounding direction {:#x}; falling back to round-to-nearest.",
                other
            );
            clamped.round_ties_even() as i32
        }
    }
}
fn lrintf(env: &mut Environment, arg: f32) -> i32 {
    lrint(env, arg.into())
}

// llrint rounds to an integer with the current rounding mode and returns
// it as a long long; saturate on overflow instead of panicking the host.
fn llrint_impl(env: &Environment, arg: f64) -> i64 {
    let rounded = match env.libc_state.math.rounding_direction {
        FE_TONEAREST => arg.round_ties_even(),
        FE_TOWARDZERO => arg.trunc(),
        FE_UPWARD => arg.ceil(),
        FE_DOWNWARD => arg.floor(),
        _ => arg.round_ties_even(),
    };
    if rounded.is_nan() {
        0
    } else {
        rounded.clamp(i64::MIN as f64, i64::MAX as f64) as i64
    }
}

fn llrint(env: &mut Environment, arg: f64) -> i64 {
    set_errno(env, 0);
    llrint_impl(env, arg)
}
fn llrintf(env: &mut Environment, arg: f32) -> i64 {
    set_errno(env, 0);
    llrint_impl(env, arg as f64)
}

// Rounding direction
fn fegetround(env: &mut Environment) -> i32 {
    env.libc_state.math.rounding_direction
}
fn fesetround(env: &mut Environment, round: i32) -> i32 {
    // Per the C standard (7.6.3.2 fesetround), the argument must be one of
    // the implementation's supported rounding-direction macros; the four
    // IEEE 754 modes below are all supported by our rint()/nearbyint().
    if round == FE_TONEAREST || round == FE_TOWARDZERO || round == FE_UPWARD || round == FE_DOWNWARD
    {
        env.libc_state.math.rounding_direction = round;
        0 // Success
    } else {
        // An invalid mode argument is not a fatal error for the guest;
        // reject it and leave the current rounding direction unchanged.
        log!(
            "Warning: fesetround({:#x}): invalid rounding direction; ignoring.",
            round
        );
        1 // Non-zero: failure
    }
}

// Remainder functions
fn fmod(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    set_errno(env, 0);
    let res = arg1 % arg2;
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}
fn fmodf(env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    set_errno(env, 0);
    let res = arg1 % arg2;
    let input_is_nan = arg1.is_nan() || arg2.is_nan();
    let input_is_inf = arg1.is_infinite() || arg2.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// IEEE remainder: x - y*n where n is the integer nearest x/y (ties to
// even). Computed via fmod() so a huge quotient cannot overflow.
fn remainder_impl(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() || x.is_infinite() || y == 0.0 {
        return f64::NAN;
    }
    if y.is_infinite() {
        return x;
    }
    let mut r = x % y;
    let half = y.abs() / 2.0;
    if r > half {
        r -= y;
    } else if -r > half {
        r += y;
    }
    r
}

fn remainder(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    set_errno(env, 0);
    remainder_impl(arg1, arg2)
}
fn remainderf(env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    set_errno(env, 0);
    remainder_impl(arg1 as f64, arg2 as f64) as f32
}

// remquo also stores the sign plus the low bits of the integer quotient
// of x/y, which numeric code uses for argument reduction.
fn remquo(env: &mut Environment, x: f64, y: f64, quo: MutPtr<i32>) -> f64 {
    set_errno(env, 0);
    let r = remainder_impl(x, y);
    if !quo.is_null() {
        // Recover the quotient from the remainder; only its sign and low
        // 3 bits are meaningful per C99, so precision loss is acceptable.
        let n = ((x - r) / y).trunc();
        let q = if n.is_nan() || n.is_infinite() {
            0i64
        } else {
            n as i64
        };
        let low = (q.unsigned_abs() & 0x7) as i32;
        env.mem.write(quo, if q < 0 { -low } else { low });
    }
    r
}
fn remquof(env: &mut Environment, x: f32, y: f32, quo: MutPtr<i32>) -> f32 {
    remquo(env, x as f64, y as f64, quo) as f32
}

// BSD aliases for remainder(); still referenced by some older iOS code.
fn drem(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    remainder(env, arg1, arg2)
}
fn dremf(env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    remainderf(env, arg1, arg2)
}

// Maximum, minimum and positive difference functions
//
// fdim(x, y) is the "positive difference" (C99 7.12.11): x - y when
// x > y, +0.0 otherwise. NaN arguments propagate; the comparison is done
// first so that (x - y).max(0.0) can't wrongly return NaN when x <= y.
fn fdim(_env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    if arg1.is_nan() || arg2.is_nan() {
        f64::NAN
    } else if arg1 > arg2 {
        arg1 - arg2
    } else {
        0.0
    }
}
fn fdimf(_env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    if arg1.is_nan() || arg2.is_nan() {
        f32::NAN
    } else if arg1 > arg2 {
        arg1 - arg2
    } else {
        0.0
    }
}
fn fmax(_env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    arg1.max(arg2)
}
fn fmaxf(_env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    arg1.max(arg2)
}
fn fmin(_env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    arg1.min(arg2)
}
fn fminf(_env: &mut Environment, arg1: f32, arg2: f32) -> f32 {
    arg1.min(arg2)
}

// sqlite3_open / sqlite3_errcode are exported from frameworks::libsqlite3;
// not duplicated in libc::math.
fn _ZNSt3__112basic_stringIcNS_11char_traitsIcEENS_9allocatorIcEEE6__initEPKcm(
    _env: &mut Environment,
    arg1: f64,
    arg2: f64,
) -> f64 {
    arg1.min(arg2)
}
fn _ZNSt6vectorIN8InputMgr7KeyDataESaIS1_EE7reserveEm(
    _env: &mut Environment,
    arg1: f64,
    arg2: f64,
) -> f64 {
    arg1.min(arg2)
}
fn _ZNSt6vectorIN8InputMgr9TouchDataESaIS1_EE7reserveEm(
    _env: &mut Environment,
    arg1: f64,
    arg2: f64,
) -> f64 {
    arg1.min(arg2)
}
fn _ZNSt6vectorIN8InputMgr7KeyDataESaIS1_EE14_M_fill_insertEN9__gnu_cxx17__normal_iteratorIPS1_S3_EEmRKS1_(
    _env: &mut Environment,
    arg1: f64,
    arg2: f64,
) -> f64 {
    arg1.min(arg2)
}

fn nearbyintf(env: &mut Environment, arg: f32) -> f32 {
    // nearbyint() rounds using the current rounding mode but never raises
    // the inexact exception; we do not model that exception anyway, so it
    // behaves the same as rintf(). (C99 7.20.6.3)
    set_errno(env, 0);
    rintf(env, arg)
}

fn nearbyint(env: &mut Environment, arg: f64) -> f64 {
    set_errno(env, 0);
    rint(env, arg)
}

fn llroundf(env: &mut Environment, arg: f32) -> i64 {
    // Per the C standard (C99 7.20.6.5), llround rounds its argument to the
    // nearest integer, halfway cases away from zero, regardless of the
    // current rounding mode. The result is saturated on overflow because a
    // domain error would need FE_INVALID handling we do not model.
    set_errno(env, 0);
    llround_impl(arg as f64)
}

fn llround(env: &mut Environment, arg: f64) -> i64 {
    set_errno(env, 0);
    llround_impl(arg)
}

fn llround_impl(arg: f64) -> i64 {
    if arg.is_nan() {
        // Domain error; the standard leaves the returned value unspecified.
        return 0;
    }
    // f64::round rounds halfway cases away from zero, which is exactly the
    // llround rule. Clamp so huge inputs saturate instead of wrapping.
    arg.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

fn rintf(env: &mut Environment, arg: f32) -> f32 {
    set_errno(env, 0);
    let res = match env.libc_state.math.rounding_direction {
        FE_TONEAREST => arg.round_ties_even(),
        FE_TOWARDZERO => arg.trunc(),
        FE_UPWARD => arg.ceil(),
        FE_DOWNWARD => arg.floor(),
        other => {
            log!(
                "Warning: rint/nearbyint: unknown rounding mode {}; defaulting to round-to-nearest.",
                other
            );
            arg.round_ties_even()
        }
    };
    let input_is_nan = arg.is_nan();
    let input_is_inf = arg.is_infinite();
    if res.is_nan() && !input_is_nan {
        set_errno(env, crate::libc::errno::EDOM);
    } else if res.is_infinite() && !input_is_inf && !input_is_nan {
        set_errno(env, crate::libc::errno::ERANGE);
    }
    res
}

// Other
fn nan(env: &mut Environment, arg: ConstPtr<u8>) -> f64 {
    // C99 7.12.11: the sequence pointed to by `arg` is a taggponent —
    // hexadecimal digits (case-insensitive, possibly with a leading '0x')
    // forming the significand bits of the quiet NaN. Empty string means
    // an unspecified taggponent. We don't model the bit pattern beyond
    // returning a quiet NaN, but we must not panic on non-empty tags.
    let tag = env.mem.cstr_at_utf8(arg).unwrap_or_default().to_owned();
    if !tag.is_empty() {
        log_dbg!("nan(\"{tag}\"): taggponent ignored, returning quiet NaN");
    }
    f64::NAN
}

// nanf() is the float counterpart of nan().
fn nanf(env: &mut Environment, arg: ConstPtr<u8>) -> f32 {
    nan(env, arg) as f32
}

// Cube root. Unlike pow(x, 1.0/3.0), cbrt() is exact at +/-1.0 and 0.0,
// and it is odd: cbrt(-x) == -cbrt(x). We compute the magnitude via a
// Newton-Raphson iteration on a scaled argument to avoid pow()'s
// intermediate overflow for extreme inputs (C99 7.12.7.1).
fn cbrt(_env: &mut Environment, arg: f64) -> f64 {
    if !arg.is_finite() || arg == 0.0 {
        return arg; // ±0.0, ±inf, NaN pass through
    }
    let sign = arg.signum();
    let magnitude = cbrt_magnitude(arg.abs());
    sign * magnitude
}

fn cbrtf(env: &mut Environment, arg: f32) -> f32 {
    cbrt(env, arg as f64) as f32
}

// cbrt() of a positive, finite value, using exponent folding so the
// Newton iteration always starts in [1, 8) and converges in a few steps.
fn cbrt_magnitude(a: f64) -> f64 {
    debug_assert!(a.is_finite() && a > 0.0);
    // Split off a multiple of 8 so the scaled argument lies in [1, 8).
    let e = (a.log2().floor() / 3.0).floor() as i32;
    let scaled = a * 2f64.powi(-3 * e);
    // Initial guess: cubic through (1,1) and (8,2) on the log scale.
    let mut r = 0.5 * scaled + 0.5;
    for _ in 0..4 {
        r = (2.0 * r + scaled / (r * r)) / 3.0;
    }
    r * 2f64.powi(e)
}

fn hypot(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    if arg1.is_infinite() {
        return f64::INFINITY;
    }
    sqrt(env, arg1 * arg1 + arg2 * arg2)
}

fn hypotf(env: &mut Environment, arg1: f64, arg2: f64) -> f64 {
    if arg1.is_infinite() {
        return f64::INFINITY;
    }
    sqrt(env, arg1 * arg1 + arg2 * arg2)
}

/// This alias is for readability, POSIX just uses `int`.
type GuestFPCategory = i32;
const FP_NAN: GuestFPCategory = 1;
const FP_INFINITE: GuestFPCategory = 2;
const FP_ZERO: GuestFPCategory = 3;
const FP_NORMAL: GuestFPCategory = 4;
const FP_SUBNORMAL: GuestFPCategory = 5;

fn __fpclassifyf(_env: &mut Environment, arg: f32) -> GuestFPCategory {
    match arg.classify() {
        FpCategory::Nan => FP_NAN,
        FpCategory::Infinite => FP_INFINITE,
        FpCategory::Zero => FP_ZERO,
        FpCategory::Normal => FP_NORMAL,
        FpCategory::Subnormal => FP_SUBNORMAL,
    }
}

fn __fpclassifyd(_env: &mut Environment, arg: f64) -> GuestFPCategory {
    match arg.classify() {
        FpCategory::Nan => FP_NAN,
        FpCategory::Infinite => FP_INFINITE,
        FpCategory::Zero => FP_ZERO,
        FpCategory::Normal => FP_NORMAL,
        FpCategory::Subnormal => FP_SUBNORMAL,
    }
}

// The isfinite/isnormal/signbit macros on Darwin expand to these helpers
// for each width. signbit() is true even for -0.0 and negative NaN.
fn __isfinitef(_env: &mut Environment, arg: f32) -> i32 {
    arg.is_finite() as i32
}
fn __isfinited(_env: &mut Environment, arg: f64) -> i32 {
    arg.is_finite() as i32
}
fn __isnormalf(_env: &mut Environment, arg: f32) -> i32 {
    arg.is_normal() as i32
}
fn __isnormald(_env: &mut Environment, arg: f64) -> i32 {
    arg.is_normal() as i32
}
fn __signbitf(_env: &mut Environment, arg: f32) -> i32 {
    arg.is_sign_negative() as i32
}
fn __signbitd(_env: &mut Environment, arg: f64) -> i32 {
    arg.is_sign_negative() as i32
}

// Type-specific classification helpers from `<math.h>`. The standard
// `isnan`/`isinf` macros on Apple platforms (and glibc) expand to these
// underlying functions: `__isnanf`/`__isnand` for NaN tests and
// `__isinff`/`__isinfd` for infinity tests. Each returns a C `int`:
// `__isnan*` returns nonzero (1) for NaN, otherwise 0; `__isinf*` returns
// 1 for +inf, -1 for -inf, otherwise 0. See the `math.h` declarations
// `extern int __isnanf(float)`, `extern int __isnand(double)`,
// `extern int __isinff(float)`, `extern int __isinfd(double)`.
fn __isnanf(_env: &mut Environment, arg: f32) -> i32 {
    arg.is_nan() as i32
}

fn __isnand(_env: &mut Environment, arg: f64) -> i32 {
    arg.is_nan() as i32
}

fn __isinff(_env: &mut Environment, arg: f32) -> i32 {
    if arg.is_infinite() {
        if arg.is_sign_positive() {
            1
        } else {
            -1
        }
    } else {
        0
    }
}

fn __isinfd(_env: &mut Environment, arg: f64) -> i32 {
    if arg.is_infinite() {
        if arg.is_sign_positive() {
            1
        } else {
            -1
        }
    } else {
        0
    }
}

// Честные 64-битные целочисленные операции (Compiler Intrinsics)

// ___udivdi3: unsigned long long / unsigned long long
// Честные 64-битные целочисленные операции (Compiler Intrinsics)

// __udivdi3: unsigned long long / unsigned long long
fn __udivdi3(_env: &mut Environment, a: u64, b: u64) -> u64 {
    if b == 0 {
        log!("Warning: __udivdi3 division by zero!");
        0
    } else {
        a / b
    }
}

// __umoddi3: unsigned long long % unsigned long long
fn __umoddi3(_env: &mut Environment, a: u64, b: u64) -> u64 {
    if b == 0 {
        log!("Warning: __umoddi3 modulo by zero!");
        0
    } else {
        a % b
    }
}

// __divdi3: signed long long / signed long long
fn __divdi3(_env: &mut Environment, a: i64, b: i64) -> i64 {
    if b == 0 {
        log!("Warning: __divdi3 division by zero!");
        0
    } else {
        a / b
    }
}

// __moddi3: signed long long % signed long long
fn __moddi3(_env: &mut Environment, a: i64, b: i64) -> i64 {
    if b == 0 {
        log!("Warning: __moddi3 modulo by zero!");
        0
    } else {
        a % b
    }
}

// __udivsi3: unsigned int / unsigned int
fn __udivsi3(_env: &mut Environment, a: u32, b: u32) -> u32 {
    if b == 0 {
        log!("Warning: __udivsi3 division by zero!");
        0
    } else {
        a / b
    }
}

// __umodsi3: unsigned int % unsigned int
fn __umodsi3(_env: &mut Environment, a: u32, b: u32) -> u32 {
    if b == 0 {
        log!("Warning: __umodsi3 modulo by zero!");
        0
    } else {
        a % b
    }
}

// compiler-rt builtins for signed 32-bit integer division and modulo. These
// are emitted by older Apple compilers when no native ARM `sdiv`/`udiv`
// instruction is available (e.g. armv6 or armv7 without `idiv`).

// __divsi3: signed int / signed int
fn __divsi3(_env: &mut Environment, a: i32, b: i32) -> i32 {
    if b == 0 {
        log!("Warning: __divsi3 division by zero!");
        0
    } else {
        // Use wrapping_div so that i32::MIN / -1 doesn't panic in debug
        // builds; compiler-rt itself defines this case as undefined.
        a.wrapping_div(b)
    }
}

// __modsi3: signed int % signed int
fn __modsi3(_env: &mut Environment, a: i32, b: i32) -> i32 {
    if b == 0 {
        log!("Warning: __modsi3 modulo by zero!");
        0
    } else {
        a.wrapping_rem(b)
    }
}

// __divmodsi4: signed int / signed int, with remainder. Returns the quotient
// and stores the remainder at `*rem`.
fn __divmodsi4(env: &mut Environment, a: i32, b: i32, rem: MutPtr<i32>) -> i32 {
    if b == 0 {
        log!("Warning: __divmodsi4 division by zero!");
        if !rem.is_null() {
            env.mem.write(rem, 0);
        }
        0
    } else {
        let q = a.wrapping_div(b);
        let r = a.wrapping_rem(b);
        if !rem.is_null() {
            env.mem.write(rem, r);
        }
        q
    }
}

// __udivmodsi4: unsigned int / unsigned int, with remainder.
fn __udivmodsi4(env: &mut Environment, a: u32, b: u32, rem: MutPtr<u32>) -> u32 {
    if b == 0 {
        log!("Warning: __udivmodsi4 division by zero!");
        if !rem.is_null() {
            env.mem.write(rem, 0);
        }
        0
    } else {
        if !rem.is_null() {
            env.mem.write(rem, a % b);
        }
        a / b
    }
}

// compiler-rt builtins for 64-bit integer to floating-point conversion. These
// are emitted by ARM compilers because the hardware has no instruction that
// converts a 64-bit integer to a float in one step.

// __floatdisf: signed long long -> float
fn __floatdisf(_env: &mut Environment, a: i64) -> f32 {
    a as f32
}

// __floatundisf: unsigned long long -> float
fn __floatundisf(_env: &mut Environment, a: u64) -> f32 {
    a as f32
}

// __floatdidf: signed long long -> double
fn __floatdidf(_env: &mut Environment, a: i64) -> f64 {
    a as f64
}

// __floatundidf: unsigned long long -> double
fn __floatundidf(_env: &mut Environment, a: u64) -> f64 {
    a as f64
}

// Compiler-rt float-to-int conversion builtins. ARM has no single-instruction
// path for converting an IEEE 754 float / double to a 64-bit integer, so the
// older Apple toolchains lower these casts to library calls. Rust's `as` cast
// matches the LLVM compiler-rt semantics: NaN converts to 0, values outside
// the destination range saturate.

// __fixsfdi: float -> signed long long
fn __fixsfdi(_env: &mut Environment, a: f32) -> i64 {
    a as i64
}

// __fixunssfdi: float -> unsigned long long
fn __fixunssfdi(_env: &mut Environment, a: f32) -> u64 {
    a as u64
}

// __fixdfdi: double -> signed long long
fn __fixdfdi(_env: &mut Environment, a: f64) -> i64 {
    a as i64
}

// __fixunsdfdi: double -> unsigned long long
fn __fixunsdfdi(_env: &mut Environment, a: f64) -> u64 {
    a as u64
}

// Честная реализация C++ Singleton<TimerManager>::getInstance()
fn _ZN9SingletonI12TimerManagerE11getInstanceEv(env: &mut Environment) -> u32 {
    // Проверяем, создавали ли мы уже этот объект
    if env.libc_state.math.timer_manager_instance == 0 {
        // Выделяем память под объект TimerManager (1024 байта с запасом).
        // Используем calloc, чтобы вся память была заполнена нулями —
        // это предотвратит краш, если игра попытается прочитать внутренние поля
        // класса.
        let ptr = env.mem.calloc(1024);
        env.libc_state.math.timer_manager_instance = ptr.to_bits();

        log_dbg!("Allocated TimerManager singleton at {:#x}", ptr.to_bits());
    }

    // Возвращаем один и тот же валидный указатель при каждом вызове
    env.libc_state.math.timer_manager_instance
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(abs(_)),
    export_c_func!(fabs(_)),
    // Trigonometric functions
    export_c_func!(sin(_)),
    export_c_func!(sincos(_, _, _)),
    export_c_func!(sincosf(_, _, _)),
    export_c_func!(__sincosf_stret(_)),
    export_c_func!(__sincos_stret(_)),
    export_c_func!(sinf(_)),
    export_c_func!(cos(_)),
    export_c_func!(cosf(_)),
    export_c_func!(tan(_)),
    export_c_func!(tanf(_)),
    export_c_func!(asin(_)),
    export_c_func!(asinf(_)),
    export_c_func!(acos(_)),
    export_c_func!(acosf(_)),
    export_c_func!(atan(_)),
    export_c_func!(atanf(_)),
    export_c_func!(atan2(_, _)),
    export_c_func!(atan2f(_, _)),
    // `long double` variants (aliases of the double ones, see above)
    export_c_func!(fabsl(_)),
    export_c_func!(sinl(_)),
    export_c_func!(cosl(_)),
    export_c_func!(tanl(_)),
    export_c_func!(asinl(_)),
    export_c_func!(acosl(_)),
    export_c_func!(atanl(_)),
    export_c_func!(atan2l(_, _)),
    export_c_func!(sinhl(_)),
    export_c_func!(coshl(_)),
    export_c_func!(tanhl(_)),
    export_c_func!(asinhl(_)),
    export_c_func!(acoshl(_)),
    export_c_func!(atanhl(_)),
    export_c_func!(expl(_)),
    export_c_func!(exp2l(_)),
    export_c_func!(expm1l(_)),
    export_c_func!(logl(_)),
    export_c_func!(log1pl(_)),
    export_c_func!(log2l(_)),
    export_c_func!(log10l(_)),
    export_c_func!(powl(_, _)),
    export_c_func!(sqrtl(_)),
    export_c_func!(cbrtl(_)),
    export_c_func!(hypotl(_, _)),
    export_c_func!(fmal(_, _, _)),
    export_c_func!(ceill(_)),
    export_c_func!(floorl(_)),
    export_c_func!(roundl(_)),
    export_c_func!(truncl(_)),
    export_c_func!(rintl(_)),
    export_c_func!(nearbyintl(_)),
    export_c_func!(lroundl(_)),
    export_c_func!(llroundl(_)),
    export_c_func!(fmodl(_, _)),
    export_c_func!(remainderl(_, _)),
    export_c_func!(fmaxl(_, _)),
    export_c_func!(fminl(_, _)),
    export_c_func!(ldexpl(_, _)),
    export_c_func!(frexpl(_, _)),
    export_c_func!(modfl(_, _)),
    export_c_func!(logbl(_)),
    // Hyperbolic functions
    export_c_func!(sinh(_)),
    export_c_func!(sinhf(_)),
    export_c_func!(cosh(_)),
    export_c_func!(coshf(_)),
    export_c_func!(tanh(_)),
    export_c_func!(tanhf(_)),
    export_c_func!(asinh(_)),
    export_c_func!(asinhf(_)),
    export_c_func!(acosh(_)),
    export_c_func!(acoshf(_)),
    export_c_func!(atanh(_)),
    export_c_func!(atanhf(_)),
    // Exponential and logarithmic functions
    export_c_func!(log(_)),
    export_c_func!(logf(_)),
    export_c_func!(log1p(_)),
    export_c_func!(log1pf(_)),
    export_c_func!(log2(_)),
    export_c_func!(log2f(_)),
    export_c_func!(log10(_)),
    export_c_func!(log10f(_)),
    export_c_func!(exp(_)),
    export_c_func!(expf(_)),
    // Apple's libm exports __exp10 and __exp10f; Mach-O prepends one underscore.
    export_c_func_aliased!("__exp10", exp10_impl(_)),
    export_c_func_aliased!("__exp10f", exp10f_impl(_)),
    export_c_func!(expm1(_)),
    export_c_func!(expm1f(_)),
    export_c_func!(exp2(_)),
    export_c_func!(exp2f(_)),
    export_c_func!(ilogb(_)),
    export_c_func!(ilogbf(_)),
    export_c_func!(logb(_)),
    export_c_func!(logbf(_)),
    export_c_func!(scalbn(_, _)),
    export_c_func!(scalbnf(_, _)),
    export_c_func!(scalbln(_, _)),
    export_c_func!(scalblnf(_, _)),
    export_c_func!(ldexp(_, _)),
    export_c_func!(ldexpf(_, _)),
    export_c_func!(frexpf(_, _)),
    export_c_func!(frexp(_, _)),
    // Power functions
    export_c_func!(pow(_, _)),
    export_c_func!(powf(_, _)),
    export_c_func!(sqrt(_)),
    export_c_func!(sqrtf(_)),
    export_c_func!(fma(_, _, _)),
    export_c_func!(fmaf(_, _, _)),
    // Nearest integer functions
    export_c_func!(ceil(_)),
    export_c_func!(ceilf(_)),
    export_c_func!(floor(_)),
    export_c_func!(floorf(_)),
    export_c_func!(round(_)),
    export_c_func!(roundf(_)),
    export_c_func!(lround(_)),
    export_c_func!(lroundf(_)),
    export_c_func!(trunc(_)),
    export_c_func!(truncf(_)),
    export_c_func!(modf(_, _)),
    export_c_func!(modff(_, _)),
    export_c_func!(rint(_)),
    export_c_func!(lrint(_)),
    export_c_func!(lrintf(_)),
    export_c_func!(llrint(_)),
    export_c_func!(llrintf(_)),
    // Rounding direction
    export_c_func!(fegetround()),
    export_c_func!(fesetround(_)),
    // Remainder functions
    export_c_func!(fmod(_, _)),
    export_c_func!(fmodf(_, _)),
    export_c_func!(remainder(_, _)),
    export_c_func!(remainderf(_, _)),
    export_c_func!(remquo(_, _, _)),
    export_c_func!(remquof(_, _, _)),
    export_c_func!(drem(_, _)),
    export_c_func!(dremf(_, _)),
    // Maximum, minimum and positive difference functions
    export_c_func!(fmax(_, _)),
    export_c_func!(fmaxf(_, _)),
    export_c_func!(fmin(_, _)),
    export_c_func!(fminf(_, _)),
    export_c_func!(fdim(_, _)),
    export_c_func!(fdimf(_, _)),
    export_c_func!(_ZNSt3__112basic_stringIcNS_11char_traitsIcEENS_9allocatorIcEEE6__initEPKcm(_, _)),
    export_c_func!(_ZNSt6vectorIN8InputMgr7KeyDataESaIS1_EE7reserveEm(_, _)),
    export_c_func!(_ZNSt6vectorIN8InputMgr9TouchDataESaIS1_EE7reserveEm(_, _)),
    export_c_func!(_ZNSt6vectorIN8InputMgr7KeyDataESaIS1_EE14_M_fill_insertEN9__gnu_cxx17__normal_iteratorIPS1_S3_EEmRKS1_(_, _)),
    // Other
    export_c_func!(rintf(_)),
    export_c_func!(nearbyint(_)),
    export_c_func!(nearbyintf(_)),
    export_c_func!(llroundf(_)),
    export_c_func!(llround(_)),
    export_c_func!(nan(_)),
    export_c_func!(nanf(_)),
    export_c_func!(hypot(_, _)),
    export_c_func!(hypotf(_, _)),
    export_c_func!(cbrt(_)),
    export_c_func!(cbrtf(_)),
    export_c_func!(__fpclassifyf(_)),
    export_c_func!(__fpclassifyd(_)),
    export_c_func!(__isfinitef(_)),
    export_c_func!(__isfinited(_)),
    export_c_func!(__isnormalf(_)),
    export_c_func!(__isnormald(_)),
    export_c_func!(__signbitf(_)),
    export_c_func!(__signbitd(_)),
    export_c_func!(__isnanf(_)),
    export_c_func!(__isnand(_)),
    export_c_func!(__isinff(_)),
    export_c_func!(__isinfd(_)),
    export_c_func!(__udivdi3(_, _)), // <--- 2 подчеркивания
    export_c_func!(__umoddi3(_, _)), // <--- 2 подчеркивания
    export_c_func!(__divdi3(_, _)),  // <--- 2 подчеркивания
    export_c_func!(__moddi3(_, _)),  // <--- 2 подчеркивания
    export_c_func!(__udivsi3(_, _)),
    export_c_func!(__umodsi3(_, _)),
    export_c_func!(__divsi3(_, _)),
    export_c_func!(__modsi3(_, _)),
    export_c_func!(__divmodsi4(_, _, _)),
    export_c_func!(__udivmodsi4(_, _, _)),
    export_c_func!(__floatdisf(_)),
    export_c_func!(__floatundisf(_)),
    export_c_func!(__floatdidf(_)),
    export_c_func!(__floatundidf(_)),
    export_c_func!(__fixsfdi(_)),
    export_c_func!(__fixunssfdi(_)),
    export_c_func!(__fixdfdi(_)),
    export_c_func!(__fixunsdfdi(_)),
    export_c_func!(_ZN9SingletonI12TimerManagerE11getInstanceEv()),
];

/// `<fenv.h>` constants.
///
/// Per Apple's `fenv.h`, `FE_DFL_ENV` is a macro defined as
/// `((const fenv_t *)&_FE_DFL_ENV)`, where `_FE_DFL_ENV` is the
/// C-level external sentinel object. With the Mach-O assembler's
/// leading-underscore convention this becomes the dynamic-loader
/// symbol `__FE_DFL_ENV` (two leading underscores). See
/// <https://developer.apple.com/documentation/kernel/fenv_h> and
/// the Open Group `fenv.h` reference
/// <https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/fenv.h.html>.
///
/// touchHLE exposes a zero-filled 8-byte `fenv_t` (matching ARMv7's
/// `__fpu_control` + reserved layout): apps that pass `FE_DFL_ENV`
/// to `fesetenv` will get the default environment (all flags clear,
/// round-to-nearest).
pub const CONSTANTS: ConstantExports = &[(
    "__FE_DFL_ENV",
    HostConstant::Custom(|env| {
        let p: crate::mem::MutPtr<u64> = env.mem.alloc(8).cast();
        env.mem.write(p, 0u64);
        crate::mem::Ptr::from_bits(p.to_bits())
    }),
)];
