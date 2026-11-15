//! regsvr32 entry points: COM class under HKCR and the TSF profile/categories.

use windows::core::{GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, E_FAIL, S_OK, WIN32_ERROR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, ITfCategoryMgr, ITfInputProcessorProfileMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, GUID_TFCAT_TIP_KEYBOARD,
};

use crate::log;

pub const CLSID_NULLIME: GUID = GUID::from_u128(0x5c9c59b0_6317_41ff_97f6_e0fe90a0c469);
pub const PROFILE_NULLIME: GUID = GUID::from_u128(0x1df3c044_b2f2_4932_9a6e_64f905123554);

/// ko-KR: the single profile per spec default ("한국어(ko-KR) 입력 프로필 하나").
const LANGID_KO_KR: u16 = 0x0412;
const DESCRIPTION: &str = "NullIME";

const CATEGORIES: [GUID; 4] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    // Lets the TIP load in Store/immersive apps and the Windows 8+ tray indicator.
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
];

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    let result = with_com(|| {
        register_class()?;
        register_profile()?;
        register_categories()
    });
    match result {
        Ok(()) => S_OK,
        Err(e) => {
            log(&format!("DllRegisterServer failed: {e}"));
            // Leave no half-registered TIP behind.
            let _ = with_com(unregister);
            e.code()
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match with_com(unregister) {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

fn unregister() -> windows::core::Result<()> {
    // Best effort for every part so a partial install can still be removed.
    let profile = unsafe {
        CoCreateInstance::<_, ITfInputProcessorProfileMgr>(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
            .and_then(|mgr| mgr.UnregisterProfile(&CLSID_NULLIME, LANGID_KO_KR, &PROFILE_NULLIME, 0))
    };
    if let Err(e) = &profile {
        log(&format!("UnregisterProfile: {e}"));
    }
    if let Ok(cats) = unsafe { CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) } {
        for cat in &CATEGORIES {
            let _ = unsafe { cats.UnregisterCategory(&CLSID_NULLIME, cat, &CLSID_NULLIME) };
        }
    }
    let key = HSTRING::from(clsid_key());
    match unsafe { RegDeleteTreeW(HKEY_CLASSES_ROOT, &key) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        e => Err(e.to_hresult().into()),
    }
}

fn with_com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    // regsvr32 initializes COM itself; S_FALSE here still needs the matching uninit.
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let result = f();
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    result
}

fn clsid_key() -> String {
    format!("CLSID\\{{{CLSID_NULLIME:?}}}")
}

pub(crate) fn dll_path() -> windows::core::Result<Vec<u16>> {
    let mut buf = vec![0u16; 1024];
    let len = unsafe { GetModuleFileNameW(Some(crate::module()), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        return Err(E_FAIL.into());
    }
    buf.truncate(len);
    Ok(buf)
}

fn register_class() -> windows::core::Result<()> {
    let path = dll_path()?;
    let class = create_key(&clsid_key())?;
    let result = set_string(class, None, &wide(DESCRIPTION));
    unsafe { RegCloseKey(class).ok()? };
    result?;
    let server = create_key(&format!("{}\\InprocServer32", clsid_key()))?;
    let result = set_string(server, None, &path).and_then(|_| set_string(server, Some("ThreadingModel"), &wide("Apartment")));
    unsafe { RegCloseKey(server).ok()? };
    result
}

fn register_profile() -> windows::core::Result<()> {
    let path = dll_path()?;
    unsafe {
        let mgr: ITfInputProcessorProfileMgr = CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        // No icon resource yet: TSF shows the language's default indicator.
        mgr.RegisterProfile(&CLSID_NULLIME, LANGID_KO_KR, &PROFILE_NULLIME, &wide(DESCRIPTION), &path, 0, HKL::default(), 0, true, 0)
    }
}

fn register_categories() -> windows::core::Result<()> {
    let cats: ITfCategoryMgr = unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
    for cat in &CATEGORIES {
        unsafe { cats.RegisterCategory(&CLSID_NULLIME, cat, &CLSID_NULLIME)? };
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn create_key(path: &str) -> windows::core::Result<HKEY> {
    let mut key = HKEY::default();
    let path = HSTRING::from(path);
    unsafe { RegCreateKeyExW(HKEY_CLASSES_ROOT, &path, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).ok()? };
    Ok(key)
}

fn set_string(key: HKEY, name: Option<&str>, value: &[u16]) -> windows::core::Result<()> {
    let name = name.map(HSTRING::from);
    let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let mut data: Vec<u8> = value.iter().flat_map(|c| c.to_le_bytes()).collect();
    data.extend_from_slice(&[0, 0]);
    let status: WIN32_ERROR = unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(&data)) };
    status.ok()
}
