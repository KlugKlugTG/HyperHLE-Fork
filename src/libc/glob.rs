/*
 * Реализация POSIX glob для HyperHLE
 */

use crate::dyld::FunctionExports;
use crate::environment::Environment;
use crate::export_c_func;
use crate::mem::{ConstPtr, MutPtr, SafeRead};

// Точная структура glob_t для 32-битного ARM (iOS/macOS)
#[repr(C)]
pub struct GuestGlobT {
    pub gl_pathc: u32,
    pub gl_matchc: i32,
    pub gl_offs: u32,
    pub gl_flags: i32,
    pub gl_pathv: MutPtr<MutPtr<u8>>,
    pub gl_errfunc: MutPtr<()>,
    pub gl_closedir: MutPtr<()>,
    pub gl_readdir: MutPtr<()>,
    pub gl_opendir: MutPtr<()>,
    pub gl_lstat: MutPtr<()>,
    pub gl_stat: MutPtr<()>,
}

unsafe impl SafeRead for GuestGlobT {}

const GLOB_SUCCESS: i32 = 0;
const GLOB_NOMATCH: i32 = -3; // Специфичное для Apple значение

fn glob(
    env: &mut Environment,
    pattern_ptr: ConstPtr<u8>,
    flags: i32,
    _errfunc: MutPtr<()>,
    pglob_ptr: MutPtr<GuestGlobT>,
) -> i32 {
    let pattern = env.mem.cstr_at_utf8(pattern_ptr).unwrap_or("");
    log_dbg!("glob(pattern: {:?}, flags: {:#x})", pattern, flags);

    let mut matched_paths: Vec<String> = Vec::new();

    // Базовая логика: если путь не содержит масок, мы просто возвращаем его.
    // Полноценный поиск по VFS (Virtual File System) с учетом масок (*) можно 
    // будет прикрутить позже, если игре не хватит прямых путей.
    if !pattern.contains('*') && !pattern.contains('?') {
        matched_paths.push(pattern.to_string());
    } else {
        log!("glob: Wildcards not fully supported yet, returning GLOB_NOMATCH for {}", pattern);
    }

    if matched_paths.is_empty() {
        return GLOB_NOMATCH;
    }

    let mut pglob = env.mem.read(pglob_ptr);
    pglob.gl_pathc = matched_paths.len() as u32;
    pglob.gl_matchc = matched_paths.len() as i32;

    // Выделение памяти под массив указателей (gl_pathv) + 1 для завершающего NULL
    let pathv_size = (matched_paths.len() as u32 + 1) * 4;
    let pathv_ptr: MutPtr<MutPtr<u8>> = env.mem.alloc(pathv_size).cast();

    for (i, path) in matched_paths.iter().enumerate() {
        let guest_str_ptr = env.mem.alloc_and_write_cstr(path.as_bytes());
        env.mem.write(pathv_ptr + (i as u32), guest_str_ptr);
    }

    // Записываем NULL-терминатор в конец массива
    env.mem.write(pathv_ptr + (matched_paths.len() as u32), MutPtr::null());

    pglob.gl_pathv = pathv_ptr;
    env.mem.write(pglob_ptr, pglob);

    GLOB_SUCCESS
}

fn globfree(env: &mut Environment, pglob_ptr: MutPtr<GuestGlobT>) {
    if pglob_ptr.is_null() {
        return;
    }
    let pglob = env.mem.read(pglob_ptr);
    
    // Честно освобождаем всю выделенную гостевую память
    if !pglob.gl_pathv.is_null() {
        for i in 0..pglob.gl_pathc {
            let str_ptr = env.mem.read(pglob.gl_pathv + i);
            if !str_ptr.is_null() {
                env.mem.free(str_ptr.cast());
            }
        }
        env.mem.free(pglob.gl_pathv.cast());
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(glob(_, _, _, _)),
    export_c_func!(globfree(_)),
];
