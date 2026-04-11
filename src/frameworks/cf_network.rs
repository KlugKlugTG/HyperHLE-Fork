/* src/frameworks/cf_network.rs */
use crate::dyld::{export_c_func, FunctionExports};
use crate::Environment;

// Заглушки для сети
fn CFHostCreateWithAddress(env: &mut Environment, allocator: u32, address: u32) -> u32 { 0 }
fn CFHostStartInfoResolution(env: &mut Environment, theHost: u32, info: u32, error: u32) -> bool { true }
fn CFReadStreamCreateForHTTPRequest(env: &mut Environment, alloc: u32, request: u32) -> u32 { 0 }
fn CFReadStreamOpen(env: &mut Environment, stream: u32) -> bool { true }

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFHostCreateWithAddress(_, _, _)),
    export_c_func!(CFHostStartInfoResolution(_, _, _)),
    export_c_func!(CFReadStreamCreateForHTTPRequest(_, _, _)),
    export_c_func!(CFReadStreamOpen(_, _)),
];
