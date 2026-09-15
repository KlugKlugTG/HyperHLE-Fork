/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `dirent.h`

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::FunctionExports;
use crate::fs::{FsNodeType, GuestPath};
use crate::libc::errno::{set_errno, EBADF, ENOENT};
use crate::mem::{guest_size_of, ConstPtr, MutPtr, Ptr, SafeRead};
use crate::{export_c_func, impl_GuestRet_for_large_struct, Environment};
use std::collections::HashMap;

/// This is an opaque struct and doesn't necessary
/// corresponds the Apple's one
/// TODO: match struct sizes
#[allow(clippy::upper_case_acronyms)]
pub(super) struct DIR {
    idx: usize,
}
unsafe impl SafeRead for DIR {}

// While early iOS is 32-bit system, underling file system uses 64-bit inodes!
pub const MAXPATHLEN: usize = 1024;

type DirentFileType = u8;
const DT_DIR: DirentFileType = 4;
const DT_REG: DirentFileType = 8;

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C, packed)]
pub(super) struct dirent {
    d_ino: u64,
    d_seekoff: u64,
    d_reclen: u16,
    pub(super) d_namlen: u16,
    d_type: u8,
    pub(super) d_name: [u8; MAXPATHLEN],
}
unsafe impl SafeRead for dirent {}
impl_GuestRet_for_large_struct!(dirent);

#[derive(Default)]
pub struct State {
    open_dirs: HashMap<MutPtr<DIR>, Vec<(String, FsNodeType)>>,
    read_dirs: HashMap<MutPtr<DIR>, Vec<MutPtr<dirent>>>,
}
impl State {
    fn get_mut(env: &mut Environment) -> &mut Self {
        &mut env.libc_state.dirent
    }
}

pub(super) fn opendir(env: &mut Environment, filename: ConstPtr<u8>) -> MutPtr<DIR> {
    // TODO: handle errno properly
    set_errno(env, 0);

    let path_bytes = env.mem.cstr_at(filename);
    let Ok(path_string) = std::str::from_utf8(path_bytes) else {
        log!("opendir: non-UTF8 path, returning NULL");
        set_errno(env, ENOENT);
        return Ptr::null();
    };
    let path_string = path_string.to_owned();
    log_dbg!("opendir: filename {}", path_string);
    let guest_path = GuestPath::new(&path_string);
    let is_dir = env.fs.is_dir(guest_path);
    if is_dir {
        let dir = env.mem.alloc_and_write(DIR { idx: 0 });
        log_dbg!("opendir: new DIR ptr: {:?}", dir);
        let Ok(iter) = env.fs.enumerate_with_types(guest_path) else {
            // Directory was removed between is_dir check and enumerate
            log!("opendir: directory disappeared, returning NULL");
            env.mem.free(dir.cast());
            set_errno(env, ENOENT);
            return Ptr::null();
        };
        let mut vec: Vec<(String, FsNodeType)> =
            iter.map(|(str, type_)| (str.to_string(), type_)).collect();
        // POSIX requires readdir() to return "." and ".." as the first two
        // entries of every directory.
        vec.insert(0, ("..".to_string(), FsNodeType::Directory));
        vec.insert(0, (".".to_string(), FsNodeType::Directory));
        State::get_mut(env).open_dirs.insert(dir, vec);
        State::get_mut(env).read_dirs.insert(dir, Vec::new());
        dir
    } else {
        set_errno(env, ENOENT);
        Ptr::null()
    }
}

/// Read the next entry of `dirp` (advancing its cursor) and build the guest
/// `dirent` struct value for it. Shared by [readdir] and [readdir_r].
/// Returns `None` for an unknown `dirp` or at the end of the directory.
fn next_dirent(env: &mut Environment, dirp: MutPtr<DIR>) -> Option<dirent> {
    let mut dir = env.mem.read(dirp);
    let vec = env.libc_state.dirent.open_dirs.get(&dirp)?;
    let Some((str, type_)) = vec.get(dir.idx) else {
        // End of directory. The cursor is left as-is, so further calls
        // keep reporting "no more entries" (matches Darwin behaviour).
        return None;
    };
    dir.idx += 1;
    env.mem.write(dirp, dir);

    let len = str.len();
    let d_type = match type_ {
        FsNodeType::File => DT_REG,
        FsNodeType::Directory => DT_DIR,
    };
    // Fill fields other than the name with values matching Apple's
    // dirent layout: a plausible inode (derived from the entry name),
    // the record length (actual struct size), and d_seekoff left at 0
    // (the guest is not allowed to rely on it for telldir anyway).
    let d_ino = {
        // FNV-1a hash of the name: stable fake inode per entry.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in str.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let d_reclen = guest_size_of::<dirent>() as u16;
    let mut dirent = dirent {
        d_ino,
        d_seekoff: 0,
        d_reclen,
        d_namlen: len as u16,
        d_type,
        d_name: [b'\0'; MAXPATHLEN],
    };
    dirent.d_name[..len].copy_from_slice(str.as_bytes());
    Some(dirent)
}

pub(super) fn readdir(env: &mut Environment, dirp: MutPtr<DIR>) -> MutPtr<dirent> {
    // TODO: handle errno properly
    set_errno(env, 0);

    if !env.libc_state.dirent.open_dirs.contains_key(&dirp) {
        log!("readdir: invalid DIR pointer {:?}, returning NULL", dirp);
        set_errno(env, EBADF);
        return Ptr::null();
    }
    let dir = env.mem.read(dirp);
    log_dbg!(
        "readdir: dirp {:?}, idx {}, entry '{:?}'",
        dirp,
        dir.idx,
        env.libc_state
            .dirent
            .open_dirs
            .get(&dirp)
            .and_then(|vec| vec.get(dir.idx))
    );
    let Some(dirent) = next_dirent(env, dirp) else {
        return Ptr::null();
    };
    let res = env.mem.alloc_and_write(dirent);
    env.libc_state
        .dirent
        .read_dirs
        .get_mut(&dirp)
        .unwrap()
        .push(res);
    res
}

pub(super) fn readdir_r(
    env: &mut Environment,
    dirp: MutPtr<DIR>,
    entry: MutPtr<dirent>,
    result: MutPtr<MutPtr<dirent>>,
) -> i32 {
    // TODO: handle errno properly
    set_errno(env, 0);

    if !env.libc_state.dirent.open_dirs.contains_key(&dirp) {
        log!("readdir_r: invalid DIR pointer {:?}", dirp);
        // POSIX: readdir_r returns the error number itself instead of
        // setting errno.
        return EBADF;
    }
    log_dbg!("readdir_r: dirp {:?}", dirp);
    match next_dirent(env, dirp) {
        Some(dirent) => {
            env.mem.write(entry, dirent);
            // On success *result points at the entry we just filled in.
            env.mem.write(result, entry);
        }
        // End of directory: *result is set to NULL and 0 is returned.
        None => env.mem.write(result, Ptr::null()),
    }
    0 // Success
}

pub(super) fn closedir(env: &mut Environment, dirp: MutPtr<DIR>) -> i32 {
    // TODO: handle errno properly
    set_errno(env, 0);

    log_dbg!("closedir: dirp {:?}", dirp);
    if let Some(vec) = env.libc_state.dirent.read_dirs.remove(&dirp) {
        for dirent in vec {
            env.mem.free(dirent.cast());
        }
    }
    if env.libc_state.dirent.open_dirs.remove(&dirp).is_some() {
        // this avoid double free if closedir() is called twice
        env.mem.free(dirp.cast());
    }
    0 // Success
}

fn scandir(
    env: &mut Environment,
    dirname: ConstPtr<u8>,
    list: MutPtr<MutPtr<MutPtr<dirent>>>,
    select: GuestFunction, // int (*select)(const struct dirent *)
    compar: GuestFunction, // int (*compar)(const struct dirent **, const struct dirent **)
) -> i32 {
    // TODO: handle errno properly
    set_errno(env, 0);

    let dirp = opendir(env, dirname);
    if dirp.is_null() {
        set_errno(env, ENOENT);
        return -1;
    }
    let mut next_dir_entry = readdir(env, dirp);
    let mut tmp_vec: Vec<MutPtr<dirent>> = vec![];
    while !next_dir_entry.is_null() {
        // POSIX: entries are only included if the optional select callback
        // returns non-zero (a NULL callback means "select everything").
        let mut keep = true;
        if !select.to_ptr().is_null() {
            let keep_res: i32 = select.call_from_host(env, (next_dir_entry.cast_const(),));
            keep = keep_res != 0;
        }
        if keep {
            tmp_vec.push(next_dir_entry);
        }
        next_dir_entry = readdir(env, dirp);
    }
    // POSIX: entries are sorted with the optional compar callback (e.g.
    // alphasort). A NULL callback leaves them in directory order.
    if !compar.to_ptr().is_null() {
        tmp_vec.sort_by(|&a, &b| {
            let pa: ConstPtr<MutPtr<dirent>> = env.mem.alloc_and_write(a).cast_const();
            let pb: ConstPtr<MutPtr<dirent>> = env.mem.alloc_and_write(b).cast_const();
            let res: i32 = compar.call_from_host(env, (pa, pb));
            env.mem.free(pa.cast_mut().cast());
            env.mem.free(pb.cast_mut().cast());
            res.cmp(&0)
        });
    }
    // we want to free dirp, but not entries themselves
    // so, we're not calling closedir() here
    env.libc_state.dirent.read_dirs.remove(&dirp);
    env.libc_state.dirent.open_dirs.remove(&dirp);
    env.mem.free(dirp.cast());

    let count: i32 = tmp_vec.len() as i32;
    let size = guest_size_of::<MutPtr<dirent>>() * count as u32;
    let mut output: MutPtr<MutPtr<dirent>> = env.mem.alloc(size).cast();
    env.mem.write(list, output);

    for entry in tmp_vec {
        env.mem.write(output, entry);
        output += 1;
    }

    count
}

fn rewinddir(env: &mut Environment, dirp: MutPtr<DIR>) {
    // В POSIX rewinddir ничего не возвращает (void).
    if dirp.is_null() {
        return;
    }

    // Проверяем, что директория действительно открыта и отслеживается эмулятором
    if !env.libc_state.dirent.open_dirs.contains_key(&dirp) {
        log!(
            "Warning: rewinddir called with invalid or already closed dirp: {:?}",
            dirp
        );
        return;
    }

    // Считываем структуру DIR из памяти гостя
    let mut dir = env.mem.read(dirp);

    // Сбрасываем курсор на начало
    dir.idx = 0;

    // Записываем обновленную структуру обратно в память гостя
    env.mem.write(dirp, dir);

    log_dbg!("rewinddir({:?}) - stream reset to beginning", dirp);
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(opendir(_)),
    export_c_func!(readdir(_)),
    export_c_func!(readdir_r(_, _, _)),
    export_c_func!(closedir(_)),
    export_c_func!(scandir(_, _, _, _)),
    export_c_func!(rewinddir(_)),
];
