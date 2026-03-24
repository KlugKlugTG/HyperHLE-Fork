/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `stdio.h`

use super::posix_io::{
    self, off_t, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY, STDERR_FILENO,
    STDIN_FILENO, STDOUT_FILENO,
};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::fs::GuestPath;
use crate::libc::errno::{set_errno, EBADF};
use crate::libc::string::strlen;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, Mem, MutPtr, MutVoidPtr, Ptr, SafeRead};
use crate::Environment;

use std::collections::HashMap;
use std::io::Write;

// Standard C functions

pub mod printf;

const EOF: i32 = -1;

struct FILEHostObject {
    /// `ungetc()` implementation
    pushbacks: Vec<u8>,
}

#[allow(clippy::upper_case_acronyms)]
/// C `FILE` struct. This is an opaque type in C, so the definition here is our
/// own.
struct FILE {
    fd: posix_io::FileDescriptor,
}
unsafe impl SafeRead for FILE {}

#[derive(Default)]
pub struct State {
    file_streams: HashMap<MutPtr<FILE>, FILEHostObject>,
}

impl State {
    fn get_mut(env: &mut Environment) -> &mut Self {
        &mut env.libc_state.stdio
    }

    fn get_file_host_obj_safe(
        &mut self,
        mem: &mut Mem,
        file_ptr: MutPtr<FILE>,
    ) -> Option<&mut FILEHostObject> {
        if file_ptr.is_null() {
            return None;
        }

        let FILE { fd } = mem.read(file_ptr);
        if matches!(fd, STDIN_FILENO | STDOUT_FILENO | STDERR_FILENO)
            && !self.file_streams.contains_key(&file_ptr)
        {
            self.file_streams.insert(
                file_ptr,
                FILEHostObject {
                    pushbacks: Vec::new(),
                },
            );
        }
        self.file_streams.get_mut(&file_ptr)
    }
}

#[allow(non_camel_case_types)]
type fpos_t = off_t;

fn fopen(env: &mut Environment, filename: ConstPtr<u8>, mode: ConstPtr<u8>) -> MutPtr<FILE> {
    let mode = env.mem.cstr_at(mode);
    let [basic_mode @ (b'r' | b'w' | b'a'), flags @ ..] = mode else {
        panic!(
            "Unexpected or missing fopen() mode first character: {:?}",
            mode.first()
        );
    };
    let mut plus = false;
    for &flag in flags {
        match flag {
            b'b' => (),
            b'+' => plus = true,
            other => {
                log!("Tolerating unrecognized fopen() mode flag: {:?}", other);
            }
        }
    }

    let flags = match (basic_mode, plus) {
        (b'r', false) => O_RDONLY,
        (b'r', true) => O_RDWR,
        (b'w', false) => O_WRONLY | O_CREAT | O_TRUNC,
        (b'w', true) => O_RDWR | O_CREAT | O_TRUNC,
        (b'a', false) => O_WRONLY | O_APPEND | O_CREAT,
        (b'a', true) => O_RDWR | O_APPEND | O_CREAT,
        _ => unreachable!(),
    };

    match posix_io::open_direct(env, filename, flags) {
        -1 => Ptr::null(),
        fd => {
            let res = env.mem.alloc_and_write(FILE { fd });
            assert!(!State::get_mut(env).file_streams.contains_key(&res));
            State::get_mut(env).file_streams.insert(
                res,
                FILEHostObject {
                    pushbacks: Vec::new(),
                },
            );
            res
        }
    }
}

fn fread(
    env: &mut Environment,
    mut buffer: MutVoidPtr,
    item_size: GuestUSize,
    n_items: GuestUSize,
    file_ptr: MutPtr<FILE>,
) -> GuestUSize {
    set_errno(env, 0);
    if item_size == 0 { return 0; }

    let total_size_orig = item_size.checked_mul(n_items).unwrap();
    let mut total_size = total_size_orig;

    let mut already_read = 0;
    if let Some(host_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
        if !host_obj.pushbacks.is_empty() {
            let to_copy = host_obj.pushbacks.len().min(total_size as usize);
            let offset = host_obj.pushbacks.len() - to_copy;

            let mut data = host_obj.pushbacks[offset..].to_vec();
            data.reverse();
            
            let to_copy_guest: GuestUSize = to_copy.try_into().unwrap();
            env.mem
                .bytes_at_mut(buffer.cast(), to_copy_guest)
                .copy_from_slice(&data);
            host_obj.pushbacks.truncate(offset);

            if total_size == to_copy_guest {
                return n_items;
            }
            total_size -= to_copy_guest;
            let ptr: MutPtr<u8> = buffer.cast();
            buffer = (ptr + to_copy_guest).cast();
            already_read = to_copy_guest;
        }
    } else {
        set_errno(env, EBADF);
        return 0;
    }

    let FILE { fd } = env.mem.read(file_ptr);
    match posix_io::read(env, fd, buffer, total_size) {
        -1 => already_read / item_size,
        bytes_read => {
            let bytes_read: GuestUSize = bytes_read.try_into().unwrap();
            (bytes_read + already_read) / item_size
        }
    }
}

fn fgetc(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    set_errno(env, 0);

    if let Some(host_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
        if let Some(pushback) = host_obj.pushbacks.pop() {
            return pushback.into();
        }
    } else {
        set_errno(env, EBADF);
        return EOF;
    }

    let FILE { fd } = env.mem.read(file_ptr);
    let buffer = env.mem.alloc(1);
    match posix_io::read(env, fd, buffer, 1) {
        -1 => EOF,
        bytes_read => {
            if bytes_read < 1 {
                EOF
            } else {
                let val = env.mem.read(buffer.cast::<u8>());
                val as i32
            }
        }
    }
}

fn getc(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    fgetc(env, file_ptr)
}

fn ungetc(env: &mut Environment, c: i32, file_ptr: MutPtr<FILE>) -> i32 {
    if c == EOF { return EOF; }
    
    if file_ptr.is_null() { return EOF; }
    let FILE { fd } = env.mem.read(file_ptr);

    _ = posix_io::lseek(env, fd, 0, SEEK_CUR);

    if let Some(host_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
        host_obj.pushbacks.push(c as u8);
        c
    } else {
        set_errno(env, EBADF);
        EOF
    }
}

fn fgets(
    env: &mut Environment,
    str: MutPtr<u8>,
    size: GuestUSize,
    stream: MutPtr<FILE>,
) -> MutPtr<u8> {
    if size == 0 { return Ptr::null(); }
    let mut read = 0;
    let mut tmp = str;
    while read < (size - 1) {
        let c = fgetc(env, stream);
        if c == EOF { break; }
        env.mem.write(tmp, c as u8);
        tmp += 1;
        read += 1;
        if c == b'\n' as i32 { break; }
    }

    if read == 0 {
        return Ptr::null();
    } else {
        env.mem.write(tmp, b'\0');
    }
    str
}

fn fputs(env: &mut Environment, str: ConstPtr<u8>, stream: MutPtr<FILE>) -> i32 {
    set_errno(env, 0);
    let str_len = strlen(env, str);
    if fwrite(env, str.cast(), str_len, 1, stream) == 1 {
        0
    } else {
        EOF
    }
}

fn fputc(env: &mut Environment, c: i32, stream: MutPtr<FILE>) -> i32 {
    let val = c as u8;
    let ptr = env.mem.alloc_and_write(val);
    let res = fwrite(env, ptr.cast_const().cast(), 1, 1, stream);
    env.mem.free(ptr.cast());
    if res == 1 { c } else { EOF }
}

fn putc(env: &mut Environment, c: i32, stream: MutPtr<FILE>) -> i32 {
    fputc(env, c, stream)
}

fn fwrite(
    env: &mut Environment,
    buffer: ConstVoidPtr,
    item_size: GuestUSize,
    n_items: GuestUSize,
    file_ptr: MutPtr<FILE>,
) -> GuestUSize {
    set_errno(env, 0);
    if item_size == 0 || buffer.is_null() { return 0; }

    if env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr).is_none() {
        set_errno(env, EBADF);
        return 0;
    }

    let FILE { fd } = env.mem.read(file_ptr);
    let total_size = item_size.checked_mul(n_items).unwrap();

    match fd {
        STDOUT_FILENO => {
            let buffer_slice = env.mem.bytes_at(buffer.cast(), total_size);
            match std::io::stdout().write(buffer_slice) {
                Ok(bytes_written) => (bytes_written as GuestUSize) / item_size,
                Err(_) => 0,
            }
        }
        STDERR_FILENO => {
            let buffer_slice = env.mem.bytes_at(buffer.cast(), total_size);
            match std::io::stderr().write(buffer_slice) {
                Ok(bytes_written) => (bytes_written as GuestUSize) / item_size,
                Err(_) => 0,
            }
        }
        _ => {
            match posix_io::write(env, fd, buffer, total_size) {
                -1 => 0,
                bytes_written => (bytes_written as GuestUSize) / item_size,
            }
        }
    }
}

#[allow(dead_code)]
const SEEK_SET: i32 = posix_io::SEEK_SET;
#[allow(dead_code)]
const SEEK_CUR: i32 = posix_io::SEEK_CUR;
#[allow(dead_code)]
const SEEK_END: i32 = posix_io::SEEK_END;

fn fseek(env: &mut Environment, file_ptr: MutPtr<FILE>, offset: i32, whence: i32) -> i32 {
    set_errno(env, 0);

    if file_ptr.is_null() { return -1; }
    let FILE { fd } = env.mem.read(file_ptr);

    let res = posix_io::lseek(env, fd, offset.into(), whence);
    if res == -1 {
        return -1;
    }

    if let Some(host_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
        host_obj.pushbacks.clear();
        0
    } else {
        set_errno(env, EBADF);
        -1
    }
}

fn ftell(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    set_errno(env, 0);
    if file_ptr.is_null() { return -1; }
    let FILE { fd } = env.mem.read(file_ptr);
    match posix_io::lseek(env, fd, 0, posix_io::SEEK_CUR) {
        -1 => -1,
        cur_pos => cur_pos as i32,
    }
}

fn rewind(env: &mut Environment, file_ptr: MutPtr<FILE>) {
    fseek(env, file_ptr, 0, SEEK_SET);
}

fn fclose(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    set_errno(env, 0);
    if file_ptr.is_null() { return EOF; }

    let fd = {
        if let Some(_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
            let FILE { fd } = env.mem.read(file_ptr);
            if matches!(fd, STDIN_FILENO | STDOUT_FILENO | STDERR_FILENO) {
                log!("Warning! fclose({:?}) called for standard descriptor {}.", file_ptr, fd);
            }
            fd
        } else {
            set_errno(env, EBADF);
            return EOF;
        }
    };

    State::get_mut(env).file_streams.remove(&file_ptr);
    env.mem.free(file_ptr.cast());

    match posix_io::close(env, fd) {
        0 => 0,
        _ => EOF,
    }
}

fn ferror(_env: &mut Environment, _file_ptr: MutPtr<FILE>) -> i32 {
    0
}

fn fsetpos(env: &mut Environment, file_ptr: MutPtr<FILE>, pos: ConstPtr<fpos_t>) -> i32 {
    set_errno(env, 0);
    if file_ptr.is_null() { return -1; }

    let FILE { fd } = env.mem.read(file_ptr);
    let offset = env.mem.read(pos);

    if posix_io::lseek(env, fd, offset, SEEK_SET) == -1 {
        -1
    } else {
        if let Some(host_obj) = env.libc_state.stdio.get_file_host_obj_safe(&mut env.mem, file_ptr) {
            host_obj.pushbacks.clear();
            0
        } else {
            set_errno(env, EBADF);
            -1
        }
    }
}

fn fgetpos(env: &mut Environment, file_ptr: MutPtr<FILE>, pos: MutPtr<fpos_t>) -> i32 {
    let res = ftell(env, file_ptr);
    if res == -1 { return -1; }
    env.mem.write(pos, res as fpos_t);
    0
}

fn feof(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    if file_ptr.is_null() { return 1; }
    let FILE { fd } = env.mem.read(file_ptr);
    posix_io::eof(env, fd)
}

fn clearerr(env: &mut Environment, file_ptr: MutPtr<FILE>) {
    if file_ptr.is_null() { return; }
    let FILE { fd } = env.mem.read(file_ptr);
    posix_io::clearerr(env, fd)
}

fn fflush(env: &mut Environment, file_ptr: MutPtr<FILE>) -> i32 {
    if file_ptr.is_null() { return 0; }
    let FILE { fd } = env.mem.read(file_ptr);
    posix_io::fflush(env, fd)
}

fn puts(env: &mut Environment, s: ConstPtr<u8>) -> i32 {
    let _ = std::io::stdout().write_all(env.mem.cstr_at(s));
    let _ = std::io::stdout().write_all(b"\n");
    0
}

fn putchar(_env: &mut Environment, c: u8) -> i32 {
    let _ = std::io::stdout().write(&[c]);
    c as i32
}

fn remove(env: &mut Environment, path: ConstPtr<u8>) -> i32 {
    if Ptr::is_null(path) { return -1; }
    match env.fs.remove(GuestPath::new(&env.mem.cstr_at_utf8(path).unwrap())) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

fn setbuf(_env: &mut Environment, _stream: MutPtr<FILE>, _buf: ConstPtr<u8>) {
    log!("Warning: ignoring setbuf()");
}

fn fileno(env: &mut Environment, file_ptr: MutPtr<FILE>) -> posix_io::FileDescriptor {
    if file_ptr.is_null() { return -1; }
    env.mem.read(file_ptr).fd
}

pub const CONSTANTS: ConstantExports = &[
    (
        "___stdinp",
        HostConstant::Custom(|env| -> ConstVoidPtr {
            let ptr = env.mem.alloc_and_write(FILE { fd: STDIN_FILENO });
            env.mem.alloc_and_write(ptr).cast().cast_const()
        }),
    ),
    (
        "___stdoutp",
        HostConstant::Custom(|env| -> ConstVoidPtr {
            let ptr = env.mem.alloc_and_write(FILE { fd: STDOUT_FILENO });
            env.mem.alloc_and_write(ptr).cast().cast_const()
        }),
    ),
    (
        "___stderrp",
        HostConstant::Custom(|env| -> ConstVoidPtr {
            let ptr = env.mem.alloc_and_write(FILE { fd: STDERR_FILENO });
            env.mem.alloc_and_write(ptr).cast().cast_const()
        }),
    ),
];

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(fopen(_, _)),
    export_c_func!(fread(_, _, _, _)),
    export_c_func!(fgetc(_)),
    export_c_func!(getc(_)),
    export_c_func!(ungetc(_, _)),
    export_c_func!(fgets(_, _, _)),
    export_c_func!(fputs(_, _)),
    export_c_func!(fputc(_, _)),
    export_c_func!(putc(_, _)),
    export_c_func!(fwrite(_, _, _, _)),
    export_c_func!(fseek(_, _, _)),
    export_c_func!(ftell(_)),
    export_c_func!(rewind(_)),
    export_c_func!(fsetpos(_, _)),
    export_c_func!(fgetpos(_, _)),
    export_c_func!(feof(_)),
    export_c_func!(clearerr(_)),
    export_c_func!(fflush(_)),
    export_c_func!(fclose(_)),
    export_c_func!(ferror(_)),
    export_c_func!(puts(_)),
    export_c_func!(putchar(_)),
    export_c_func!(remove(_)),
    export_c_func!(setbuf(_, _)),
    export_c_func!(fileno(_)),
];

