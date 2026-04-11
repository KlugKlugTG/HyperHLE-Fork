/* src/frameworks/core_audio.rs */
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstPtr, MutPtr};
use crate::Environment;

// Импортируем типы из файла, который у вас уже есть
use super::core_audio_types::*; 

// Заглушки функций, которые требует игра
fn AudioSessionInitialize(
    env: &mut Environment,
    inRunLoop: MutPtr<u8>,
    inRunLoopMode: ConstPtr<u8>,
    inInterruptionListener: u32,
    inClientData: MutPtr<u8>
) -> u32 { 0 }

fn AudioSessionSetActive(env: &mut Environment, active: bool) -> u32 { 0 }

fn AudioQueueNewOutput(
    env: &mut Environment,
    inFormat: ConstPtr<AudioStreamBasicDescription>,
    inCallbackProc: u32,
    inUserData: MutPtr<u8>,
    inCallbackRunLoop: MutPtr<u8>,
    inCallbackRunLoopMode: ConstPtr<u8>,
    inFlags: u32,
    outAQ: MutPtr<u32>
) -> u32 { 0 }

// Экспорт функций, чтобы эмулятор их "увидел"
pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(AudioSessionInitialize(_, _, _, _)),
    export_c_func!(AudioSessionSetActive(_, _)),
    export_c_func!(AudioQueueNewOutput(_, _, _, _, _, _, _)),
];
