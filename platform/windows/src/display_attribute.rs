//! Two composition styles: plain input underline, and a thick underline on the
//! segment that Left/Right moved to (D8).

use std::cell::Cell;
use windows::core::{implement, BSTR, GUID};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo, ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT,
    TF_ATTR_TARGET_CONVERTED, TF_DISPLAYATTRIBUTE, TF_LS_SOLID,
};

pub const GUID_ATTR_INPUT: GUID = GUID::from_u128(0x343a292d_851f_45e0_9ecd_c4d30cbda9b8);
pub const GUID_ATTR_FOCUSED: GUID = GUID::from_u128(0x73979a91_21ce_44ee_a82e_1f60be826fd6);

const ALL: [GUID; 2] = [GUID_ATTR_INPUT, GUID_ATTR_FOCUSED];

pub fn info(guid: &GUID) -> windows::core::Result<ITfDisplayAttributeInfo> {
    if ALL.contains(guid) {
        Ok(AttributeInfo(*guid).into())
    } else {
        Err(E_INVALIDARG.into())
    }
}

pub fn enumerate() -> IEnumTfDisplayAttributeInfo {
    AttributeEnum { next: Cell::new(0) }.into()
}

#[implement(ITfDisplayAttributeInfo)]
struct AttributeInfo(GUID);

impl ITfDisplayAttributeInfo_Impl for AttributeInfo_Impl {
    fn GetGUID(&self) -> windows::core::Result<GUID> {
        Ok(self.0)
    }

    fn GetDescription(&self) -> windows::core::Result<BSTR> {
        Ok(BSTR::from(if self.0 == GUID_ATTR_FOCUSED { "NullIME focused segment" } else { "NullIME input" }))
    }

    fn GetAttributeInfo(&self, out: *mut TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        if out.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let focused = self.0 == GUID_ATTR_FOCUSED;
        // Colours stay TF_CT_NONE (zeroed) so the app keeps its own text colours.
        let attr = TF_DISPLAYATTRIBUTE {
            lsStyle: TF_LS_SOLID,
            fBoldLine: focused.into(),
            bAttr: if focused { TF_ATTR_TARGET_CONVERTED } else { TF_ATTR_INPUT },
            ..Default::default()
        };
        unsafe { out.write(attr) };
        Ok(())
    }

    fn SetAttributeInfo(&self, _: *const TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

#[implement(IEnumTfDisplayAttributeInfo)]
struct AttributeEnum {
    next: Cell<usize>,
}

impl IEnumTfDisplayAttributeInfo_Impl for AttributeEnum_Impl {
    fn Clone(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        Ok(AttributeEnum { next: Cell::new(self.next.get()) }.into())
    }

    fn Next(&self, count: u32, out: *mut Option<ITfDisplayAttributeInfo>, fetched: *mut u32) -> windows::core::Result<()> {
        let mut n = 0;
        while n < count as usize && self.next.get() < ALL.len() {
            unsafe { out.add(n).write(Some(AttributeInfo(ALL[self.next.get()]).into())) };
            self.next.set(self.next.get() + 1);
            n += 1;
        }
        if !fetched.is_null() {
            unsafe { fetched.write(n as u32) };
        }
        if n < count as usize {
            // S_FALSE ends the caller's loop; windows-rs maps any Err to its HRESULT.
            return Err(S_FALSE.into());
        }
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.next.set(0);
        Ok(())
    }

    fn Skip(&self, count: u32) -> windows::core::Result<()> {
        let next = self.next.get() + count as usize;
        self.next.set(next.min(ALL.len()));
        if next > ALL.len() {
            return Err(S_FALSE.into());
        }
        Ok(())
    }
}
