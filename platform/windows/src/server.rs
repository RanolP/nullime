//! In-process COM server: class factory and the DLL lifetime exports.

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use windows::core::{implement, Interface, Ref, BOOL, GUID, HRESULT, IUnknown};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};

use crate::registration::CLSID_NULLIME;
use crate::service::TextService;

/// Live objects plus LockServer locks; DllCanUnloadNow says yes only at zero.
static LOCKS: AtomicI32 = AtomicI32::new(0);

pub fn lock() {
    LOCKS.fetch_add(1, Ordering::SeqCst);
}

pub fn unlock() {
    LOCKS.fetch_sub(1, Ordering::SeqCst);
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = TextService::new().into();
        unsafe { unknown.query(iid, object).ok() }
    }

    fn LockServer(&self, lock_server: BOOL) -> windows::core::Result<()> {
        if lock_server.as_bool() {
            lock();
        } else {
            unlock();
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, object: *mut *mut c_void) -> HRESULT {
    if clsid.is_null() || iid.is_null() || object.is_null() {
        return E_POINTER;
    }
    unsafe { *object = std::ptr::null_mut() };
    if unsafe { *clsid } != CLSID_NULLIME {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    unsafe { factory.query(iid, object) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
