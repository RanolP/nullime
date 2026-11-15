//! Writes an engine `Response` into the document inside a TSF edit session:
//! commit text, the composition (preedit) with per-segment display attributes,
//! and the candidate window position.

use std::mem::ManuallyDrop;
use windows::core::{implement, ComObject, Interface};
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, INSERT_TEXT_AT_SELECTION_FLAGS,
    GUID_PROP_ATTRIBUTE, TF_AE_NONE, TF_ANCHOR_END, TF_ANCHOR_START, TF_ES_ASYNCDONTCARE, TF_ES_READWRITE, TF_ES_SYNC, TF_E_SYNCHRONOUS,
    TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};

use nullime_engine::{Preedit, Response};

use crate::log;
use crate::service::TextService;

#[implement(ITfEditSession)]
struct ApplyResponse {
    // Holding the service keeps it alive until an async session runs.
    service: ComObject<TextService>,
    sink: ITfCompositionSink,
    context: ITfContext,
    response: Response,
}

impl ITfEditSession_Impl for ApplyResponse_Impl {
    fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
        let result = apply(&self.service, &self.sink, ec, &self.context, &self.response);
        if let Err(e) = &result {
            log(&format!("DoEditSession failed: {e}; response {:?}", self.response));
        }
        result
    }
}

/// `sync` is only granted inside key handling; elsewhere TSF answers
/// TF_E_SYNCHRONOUS and the session is queued instead.
pub fn request(service: ComObject<TextService>, sink: ITfCompositionSink, client_id: u32, context: &ITfContext, response: Response, sync: bool) {
    let session: ITfEditSession = ApplyResponse { service, sink, context: context.clone(), response }.into();
    if sync {
        match unsafe { context.RequestEditSession(client_id, &session, TF_ES_READWRITE | TF_ES_SYNC) } {
            Ok(_) => return,
            Err(e) if e.code() == TF_E_SYNCHRONOUS => {}
            Err(e) => {
                log(&format!("sync RequestEditSession: {e}"));
                return;
            }
        }
    }
    if let Err(e) = unsafe { context.RequestEditSession(client_id, &session, TF_ES_READWRITE | TF_ES_ASYNCDONTCARE) } {
        log(&format!("RequestEditSession: {e}"));
    }
}

fn apply(service: &TextService, sink: &ITfCompositionSink, ec: u32, context: &ITfContext, response: &Response) -> windows::core::Result<()> {
    let mut composition = service.composition();
    if !response.commit.is_empty() || response.preedit.is_none() {
        match composition.take() {
            Some(c) => end_composition(ec, context, &c, &response.commit)?,
            None if !response.commit.is_empty() => insert_at_selection(ec, context, &response.commit)?,
            None => {}
        }
        service.set_composition(None, None);
    }
    let Some(preedit) = &response.preedit else {
        service.hide_candidates();
        return Ok(());
    };
    let composition = match composition {
        Some(c) => c,
        None => start_composition(ec, context, sink)?,
    };
    // Stored before any further call can fail, so a later key still ends it.
    service.set_composition(Some(composition.clone()), Some(context.clone()));
    let range = unsafe { composition.GetRange()? };
    unsafe { range.SetText(ec, 0, &utf16(&preedit_text(preedit)))? };
    let focused = mark_segments(ec, context, &range, preedit, service.atoms())?;
    set_caret(ec, context, &range)?;
    match text_rect(ec, context, &focused) {
        Ok(rect) => service.show_candidates(&preedit.segments[preedit.focused], rect),
        // TS_E_NOLAYOUT and friends: the app has no layout yet; keep the old spot.
        Err(e) => log(&format!("GetTextExt: {e}")),
    }
    Ok(())
}

fn preedit_text(preedit: &Preedit) -> String {
    preedit.segments.iter().map(|s| s.text.as_str()).collect()
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn start_composition(ec: u32, context: &ITfContext, sink: &ITfCompositionSink) -> windows::core::Result<ITfComposition> {
    unsafe {
        let insert: ITfInsertAtSelection = context.cast()?;
        // QUERYONLY: the empty range at the caret (or over the selection) without editing.
        let range = insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
        context.cast::<ITfContextComposition>()?.StartComposition(ec, &range, sink)
    }
}

/// Replaces the preedit with `commit`, drops the underline, parks the caret
/// after it and closes the composition. EndComposition from the owner does not
/// call back OnCompositionTerminated.
fn end_composition(ec: u32, context: &ITfContext, composition: &ITfComposition, commit: &str) -> windows::core::Result<()> {
    unsafe {
        let range = composition.GetRange()?;
        range.SetText(ec, 0, &utf16(commit))?;
        context.GetProperty(&GUID_PROP_ATTRIBUTE)?.Clear(ec, &range)?;
        set_caret(ec, context, &range)?;
        composition.EndComposition(ec)
    }
}

fn insert_at_selection(ec: u32, context: &ITfContext, text: &str) -> windows::core::Result<()> {
    unsafe {
        let insert: ITfInsertAtSelection = context.cast()?;
        let range = insert.InsertTextAtSelection(ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &utf16(text))?;
        set_caret(ec, context, &range)
    }
}

/// Sets GUID_PROP_ATTRIBUTE per segment and returns the focused segment's range.
fn mark_segments(ec: u32, context: &ITfContext, range: &ITfRange, preedit: &Preedit, atoms: crate::service::Atoms) -> windows::core::Result<ITfRange> {
    unsafe {
        let property = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
        let mut focused = range.Clone()?;
        let mut start = 0i32;
        for (i, segment) in preedit.segments.iter().enumerate() {
            let len = segment.text.encode_utf16().count() as i32;
            let r = range.Clone()?;
            r.Collapse(ec, TF_ANCHOR_START)?;
            let mut moved = 0;
            r.ShiftEnd(ec, start + len, &mut moved, std::ptr::null())?;
            r.ShiftStart(ec, start, &mut moved, std::ptr::null())?;
            let atom = if i == preedit.focused { atoms.focused } else { atoms.input };
            property.SetValue(ec, &r, &VARIANT::from(atom as i32))?;
            if i == preedit.focused {
                focused = r;
            }
            start += len;
        }
        Ok(focused)
    }
}

fn set_caret(ec: u32, context: &ITfContext, range: &ITfRange) -> windows::core::Result<()> {
    unsafe {
        let caret = range.Clone()?;
        caret.Collapse(ec, TF_ANCHOR_END)?;
        let mut selection = TF_SELECTION {
            range: ManuallyDrop::new(Some(caret)),
            style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: false.into() },
        };
        let result = context.SetSelection(ec, std::slice::from_ref(&selection));
        ManuallyDrop::drop(&mut selection.range);
        result
    }
}

fn text_rect(ec: u32, context: &ITfContext, range: &ITfRange) -> windows::core::Result<RECT> {
    let mut rect = RECT::default();
    let mut clipped = Default::default();
    unsafe { context.GetActiveView()?.GetTextExt(ec, range, &mut rect, &mut clipped)? };
    Ok(rect)
}
