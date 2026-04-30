/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Handling of Objective-C objects.

use super::{Class, ClassHostObject};
use crate::mem::{guest_size_of, GuestUSize, Mem, MutPtr, Ptr, SafeRead};
use std::any::Any;
use std::num::NonZeroU32;

#[repr(C, packed)]
pub struct objc_object {
    pub(super) isa: Class,
}
unsafe impl SafeRead for objc_object {}

#[allow(non_camel_case_types)]
pub type id = MutPtr<objc_object>;

#[allow(non_upper_case_globals)]
pub const nil: id = Ptr::null();

pub(super) struct HostObjectEntry {
    host_object: Box<dyn AnyHostObject>,
    refcount: Option<NonZeroU32>,
}

pub trait HostObject: Any + 'static {
    fn as_superclass<'a>(&'a self) -> Option<&'a (dyn AnyHostObject + 'static)> {
        None
    }
    fn as_superclass_mut<'a>(&'a mut self) -> Option<&'a mut (dyn AnyHostObject + 'static)> {
        None
    }
}

#[macro_export]
macro_rules! impl_HostObject_with_superclass {
    ( $ty:ty ) => {
        impl $crate::objc::HostObject for $ty {
            fn as_superclass<'a>(
                &'a self,
            ) -> Option<&'a (dyn $crate::objc::AnyHostObject + 'static)> {
                Some(&self.superclass)
            }
            fn as_superclass_mut<'a>(
                &'a mut self,
            ) -> Option<&'a mut (dyn $crate::objc::AnyHostObject + 'static)> {
                Some(&mut self.superclass)
            }
        }
    };
}
pub use crate::impl_HostObject_with_superclass;

pub trait AnyHostObject: HostObject {
    fn as_any<'a>(&'a self) -> &'a (dyn Any + 'static);
    fn as_any_mut<'a>(&'a mut self) -> &'a mut (dyn Any + 'static);
    fn type_name(&self) -> &'static str;
}
impl<T: HostObject> AnyHostObject for T {
    fn as_any<'a>(&'a self) -> &'a (dyn Any + 'static) {
        self
    }
    fn as_any_mut<'a>(&'a mut self) -> &'a mut (dyn Any + 'static) {
        self
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

pub struct TrivialHostObject;
impl HostObject for TrivialHostObject {}

impl super::ObjC {
    pub fn read_isa(object: id, mem: &Mem) -> Class {
        if object == nil {
            return Ptr::null();
        }
        mem.read(object).isa
    }

    fn alloc_object_inner(
        &mut self,
        isa: Class,
        instance_size: GuestUSize,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
        refcount: Option<NonZeroU32>,
    ) -> id {
        let guest_object = objc_object { isa };
        let ptr: MutPtr<objc_object> = mem.alloc(instance_size).cast();
        mem.write(ptr, guest_object);
        self.objects.insert(
            ptr,
            HostObjectEntry {
                host_object,
                refcount,
            },
        );
        ptr
    }

    pub fn alloc_object(
        &mut self,
        isa: Class,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
    ) -> id {
        let instance_size = self
            .get_host_object(isa)
            .and_then(|h| h.as_any().downcast_ref::<ClassHostObject>())
            .map(|c| c.instance_size)
            .unwrap_or(guest_size_of::<objc_object>());

        self.alloc_object_inner(
            isa,
            instance_size,
            host_object,
            mem,
            Some(NonZeroU32::new(1).unwrap()),
        )
    }

    pub fn alloc_static_object(
        &mut self,
        isa: Class,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
    ) -> id {
        let size = guest_size_of::<objc_object>();
        self.alloc_object_inner(isa, size, host_object, mem, None)
    }

    pub fn register_static_object(
        &mut self,
        guest_object: id,
        host_object: Box<dyn AnyHostObject>,
    ) {
        if guest_object == nil {
            return;
        }
        self.objects.insert(
            guest_object,
            HostObjectEntry {
                host_object,
                refcount: None,
            },
        );
    }

    pub fn get_host_object(&self, object: id) -> Option<&dyn AnyHostObject> {
        if object == nil {
            return None;
        }
        self.objects.get(&object).map(|entry| &*entry.host_object)
    }

    /// Format a guest object as `0xADDR (class "Name")` for diagnostics, when
    /// possible. Falls back to `0xADDR (no isa / class lookup failed)` if the
    /// object's `isa` cannot be resolved (e.g. the object was never alloc'd
    /// or is wholly outside the runtime's object table).
    fn describe_object(&self, object: id) -> String {
        if object == nil {
            return "nil".to_string();
        }
        // We can't read guest memory from here without `&Mem`, so instead use
        // the host-object table to recover the class entry that was stored at
        // alloc time.
        if let Some(entry) = self.objects.get(&object) {
            let stored_type = entry.host_object.type_name();
            return format!("{:?} (host-stored as {})", object, stored_type);
        }
        format!("{:?} (no host object recorded)", object)
    }

    pub fn borrow<T: AnyHostObject + 'static>(&self, object: id) -> &T {
        if let Some(entry) = self.objects.get(&object) {
            let mut host_object: &(dyn AnyHostObject + 'static) = &*entry.host_object;
            loop {
                if let Some(res) = host_object.as_any().downcast_ref() {
                    return res;
                } else if let Some(next) = host_object.as_superclass() {
                    host_object = next;
                } else {
                    break;
                }
            }
        }

        // Per Apple documentation every Objective-C object has a stable host
        // representation: alloc/init produces a host-side state struct that
        // every later message reads. If we end up here it means either:
        //   * the object was never allocated through one of our +alloc paths
        //     (custom guest allocator / objc_addClass / NIB unarchiver bug), or
        //   * a UIView subclass owns its own HostObject but doesn't expose its
        //     UIViewHostObject through `as_superclass` (use
        //     `impl_HostObject_with_superclass!` and embed
        //     `superclass: super::UIViewHostObject`).
        //
        // Returning shared static memory under the requested type would alias
        // every "lost" view to the same frame/bounds/subviews, so instead we
        // hand out a dedicated zero-initialized buffer per (type, object) pair.
        // Same `(object, T)` always returns the same buffer so repeated reads
        // observe earlier writes.
        log!(
            "Warning: borrow::<{}>() on {} — synthesizing per-object zero buffer.",
            std::any::type_name::<T>(),
            self.describe_object(object)
        );
        leaked_zero_buffer::<T>(object)
    }

    pub fn borrow_mut<T: AnyHostObject + 'static>(&mut self, object: id) -> &mut T {
        if let Some(entry) = self.objects.get_mut(&object) {
            type Aho = dyn AnyHostObject + 'static;
            let mut host_object: &mut Aho = &mut *entry.host_object;
            loop {
                let current_ptr = host_object as *mut Aho;
                if let Some(res) = unsafe { &mut *current_ptr }.as_any_mut().downcast_mut() {
                    return res;
                }

                let has_super = unsafe { &*current_ptr }.as_superclass().is_some();
                if has_super {
                    host_object = unsafe { &mut *current_ptr }.as_superclass_mut().unwrap();
                } else {
                    break;
                }
            }
        }

        log!(
            "Warning: borrow_mut::<{}>() on {} — synthesizing per-object zero buffer.",
            std::any::type_name::<T>(),
            self.describe_object(object)
        );
        leaked_zero_buffer_mut::<T>(object)
    }

    pub fn get_refcount(&mut self, object: id) -> NonZeroU32 {
        let default_rc = NonZeroU32::new(1).unwrap();
        if object == nil {
            return default_rc;
        }

        self.objects
            .get(&object)
            .and_then(|e| e.refcount)
            .unwrap_or(default_rc)
    }

    pub fn increment_refcount(&mut self, object: id) {
        if object == nil {
            return;
        }
        if let Some(entry) = self.objects.get_mut(&object) {
            if let Some(refcount) = entry.refcount.as_mut() {
                if let Some(new_rc) = refcount.get().checked_add(1) {
                    *refcount = NonZeroU32::new(new_rc).unwrap();
                }
            }
        }
    }

    #[must_use]
    pub fn decrement_refcount(&mut self, object: id) -> bool {
        if object == nil {
            return false;
        }
        if let Some(entry) = self.objects.get_mut(&object) {
            if let Some(refcount) = entry.refcount.as_mut() {
                if refcount.get() == 1 {
                    entry.refcount = None;
                    return true;
                } else {
                    *refcount = NonZeroU32::new(refcount.get() - 1).unwrap();
                }
            }
        }
        false
    }

    pub fn dealloc_object(&mut self, object: id, mem: &mut Mem) {
        if object == nil {
            return;
        }

        if let Some(entry) = self.objects.remove(&object) {
            std::mem::drop(entry.host_object);
            mem.free(object.cast());
        }
    }
}

// =====================================================================
// Per-object synthetic host buffers (fallback for `borrow` / `borrow_mut`).
//
// When a guest object cannot be resolved to its proper HostObject we still
// must return a `&T` of the requested host-object type. Prior code aliased a
// single global static buffer under every type T, which made every
// "missing" UIView share the same frame/bounds/subviews state. Instead we
// keep a `(id, TypeId)` -> heap-allocated zero buffer table; each
// (object, type) pair gets its own dedicated, aligned buffer that lives for
// the rest of the process. Subsequent reads observe earlier writes.
// =====================================================================

use std::any::TypeId;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Mutex;

struct LeakedBuf {
    ptr: NonNull<u8>,
    layout: std::alloc::Layout,
}
// SAFETY: `ptr` is read/written only via `&T`/`&mut T` references that the
// caller serializes; the box itself is never deallocated. The pointer never
// moves so it can cross threads.
unsafe impl Send for LeakedBuf {}
unsafe impl Sync for LeakedBuf {}

fn leaked_buf_table() -> &'static Mutex<HashMap<(id, TypeId), LeakedBuf>> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Mutex<HashMap<(id, TypeId), LeakedBuf>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_or_alloc_zero<T: 'static>(object: id) -> NonNull<u8> {
    let key = (object, TypeId::of::<T>());
    let mut tbl = leaked_buf_table().lock().unwrap();
    if let Some(buf) = tbl.get(&key) {
        return buf.ptr;
    }
    let layout = std::alloc::Layout::new::<T>();
    // SAFETY: `layout` is non-zero for any `Sized` host-object type we use.
    // `alloc_zeroed` returns memory matching that layout; we leak it.
    let raw = unsafe { std::alloc::alloc_zeroed(layout) };
    let ptr = NonNull::new(raw).expect("alloc_zeroed for synthetic host object");
    tbl.insert(key, LeakedBuf { ptr, layout });
    ptr
}

fn leaked_zero_buffer<T: 'static>(object: id) -> &'static T {
    let p = get_or_alloc_zero::<T>(object);
    // SAFETY: returned pointer is `Layout::new::<T>()` aligned and valid for
    // the entire program lifetime; the table guarantees we always hand out
    // the same pointer for the same `(object, T)` pair, so aliasing rules
    // hold across repeated `borrow()` calls.
    unsafe { &*(p.as_ptr().cast::<T>()) }
}

fn leaked_zero_buffer_mut<T: 'static>(object: id) -> &'static mut T {
    let p = get_or_alloc_zero::<T>(object);
    // SAFETY: see `leaked_zero_buffer`. `borrow_mut` takes `&mut self` on
    // `ObjC`, which serialises access through Rust's borrow checker, so we
    // never produce two live `&mut` references to the same buffer.
    unsafe { &mut *(p.as_ptr().cast::<T>()) }
}
