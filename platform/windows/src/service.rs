//! The TSF text service: activation, key routing to the engine, focus changes.
//!
//! Threading: TSF calls every method on the app's UI thread (apartment model),
//! so state sits in RefCells. Each borrow is released before calling back into
//! TSF, because a synchronous edit session re-enters this object.

use std::cell::{Cell, RefCell};
use windows::core::{implement, IUnknownImpl, Interface, Ref, BOOL, GUID};
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, IEnumTfDisplayAttributeInfo, ITfCategoryMgr, ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl, ITfDocumentMgr, ITfKeyEventSink,
    ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfSource, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor_Impl, ITfThreadMgr, ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl, GUID_PROP_ATTRIBUTE,
};

use nullime_engine::{Engine, Key, KeyEvent, Response, Segment};

use crate::candidate_window::CandidateWindow;
use crate::composition;
use crate::display_attribute::{self, GUID_ATTR_FOCUSED, GUID_ATTR_INPUT};
use crate::keymap::map_key;
use crate::log;

#[derive(Clone, Copy, Default)]
pub struct Atoms {
    pub input: u32,
    pub focused: u32,
}

#[derive(Default)]
struct Session {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    thread_mgr_cookie: Option<u32>,
    key_sink_advised: bool,
    atoms: Atoms,
    candidates: Option<CandidateWindow>,
    composition: Option<ITfComposition>,
    /// The context the open composition lives in; focus changes commit into it.
    composition_context: Option<ITfContext>,
}

#[implement(ITfTextInputProcessorEx, ITfKeyEventSink, ITfThreadMgrEventSink, ITfCompositionSink, ITfDisplayAttributeProvider)]
pub struct TextService {
    engine: RefCell<Engine>,
    session: RefCell<Session>,
    /// Mirrors `Response.preedit.is_some()` of the last engine reply, so
    /// OnTestKeyDown can predict the engine without mutating it.
    composing: Cell<bool>,
}

impl TextService {
    pub fn new() -> TextService {
        crate::server::lock();
        TextService { engine: RefCell::new(crate::dictionary::engine()), session: RefCell::default(), composing: Cell::new(false) }
    }

    pub fn atoms(&self) -> Atoms {
        self.session.borrow().atoms
    }

    pub fn composition(&self) -> Option<ITfComposition> {
        self.session.borrow().composition.clone()
    }

    pub fn set_composition(&self, composition: Option<ITfComposition>, context: Option<ITfContext>) {
        let mut s = self.session.borrow_mut();
        s.composition = composition;
        s.composition_context = context;
    }

    pub fn show_candidates(&self, segment: &Segment, anchor: RECT) {
        if let Some(w) = &self.session.borrow().candidates {
            w.show(segment, anchor);
        }
    }

    pub fn hide_candidates(&self) {
        if let Some(w) = &self.session.borrow().candidates {
            w.hide();
        }
    }

    /// Whether the engine will want this key. Space/Enter/Other while composing
    /// say yes so OnKeyDown gets to commit first; OnKeyDown then reports the
    /// engine's real `handled`, which hands those keys on to the app (D5).
    fn wants(&self, ev: &KeyEvent) -> bool {
        self.composing.get() || (matches!(ev.key, Key::Char(_)) && !ev.modifier)
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::server::unlock();
    }
}

fn current_key(wparam: WPARAM) -> Option<KeyEvent> {
    let down = |vk: u16| unsafe { GetKeyState(vk as i32) } < 0;
    map_key(wparam.0 as u16, down(VK_SHIFT.0), down(VK_CONTROL.0), down(VK_MENU.0))
}

impl TextService_Impl {
    fn apply(&self, context: &ITfContext, response: Response, sync: bool) {
        self.composing.set(response.preedit.is_some());
        let client_id = self.session.borrow().client_id;
        let sink: ITfCompositionSink = self.to_interface();
        composition::request(self.to_object(), sink, client_id, context, response, sync);
    }

    /// Commits whatever is composing (focus change, deactivation).
    fn flush(&self) {
        if !self.composing.get() {
            return;
        }
        let response = self.engine.borrow_mut().reset();
        let context = self.session.borrow().composition_context.clone();
        match context {
            Some(context) => self.apply(&context, response, false),
            None => {
                self.composing.set(false);
                self.hide_candidates();
            }
        }
    }

    fn deactivate(&self) {
        self.flush();
        // Take the sinks and window out first: unadvising and DestroyWindow call
        // into TSF and the window procedure while no borrow is held. The
        // composition stays: the flush above may still be a queued session.
        let s = {
            let mut s = self.session.borrow_mut();
            let composition = s.composition.take();
            let composition_context = s.composition_context.take();
            let taken = std::mem::take(&mut *s);
            s.composition = composition;
            s.composition_context = composition_context;
            taken
        };
        if let Some(tm) = s.thread_mgr {
            if s.key_sink_advised {
                if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                    let _ = unsafe { km.UnadviseKeyEventSink(s.client_id) };
                }
            }
            if let (Some(cookie), Ok(source)) = (s.thread_mgr_cookie, tm.cast::<ITfSource>()) {
                let _ = unsafe { source.UnadviseSink(cookie) };
            }
        }
        drop(s.candidates);
    }

    fn activate(&self, thread_mgr: &ITfThreadMgr, client_id: u32) -> windows::core::Result<()> {
        {
            let mut s = self.session.borrow_mut();
            s.thread_mgr = Some(thread_mgr.clone());
            s.client_id = client_id;
        }
        let source: ITfSource = thread_mgr.cast()?;
        let cookie = unsafe { source.AdviseSink(&ITfThreadMgrEventSink::IID, &self.to_interface::<ITfThreadMgrEventSink>())? };
        self.session.borrow_mut().thread_mgr_cookie = Some(cookie);

        let km: ITfKeystrokeMgr = thread_mgr.cast()?;
        unsafe { km.AdviseKeyEventSink(client_id, &self.to_interface::<ITfKeyEventSink>(), true)? };
        self.session.borrow_mut().key_sink_advised = true;

        let cats: ITfCategoryMgr = unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
        let atoms = unsafe { Atoms { input: cats.RegisterGUID(&GUID_ATTR_INPUT)?, focused: cats.RegisterGUID(&GUID_ATTR_FOCUSED)? } };
        self.session.borrow_mut().atoms = atoms;

        let window = CandidateWindow::new()?;
        self.session.borrow_mut().candidates = Some(window);
        Ok(())
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, thread_mgr: Ref<ITfThreadMgr>, client_id: u32) -> windows::core::Result<()> {
        self.ActivateEx(thread_mgr, client_id, 0)
    }

    fn Deactivate(&self) -> windows::core::Result<()> {
        self.deactivate();
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, thread_mgr: Ref<ITfThreadMgr>, client_id: u32, _flags: u32) -> windows::core::Result<()> {
        let thread_mgr = thread_mgr.ok()?;
        let result = self.activate(thread_mgr, client_id);
        if let Err(e) = &result {
            log(&format!("Activate failed: {e}"));
            self.deactivate();
        }
        result
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(&self, _context: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> windows::core::Result<BOOL> {
        Ok(current_key(wparam).is_some_and(|ev| self.wants(&ev)).into())
    }

    fn OnTestKeyUp(&self, _context: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> windows::core::Result<BOOL> {
        Ok(false.into())
    }

    fn OnKeyDown(&self, context: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> windows::core::Result<BOOL> {
        let Some(ev) = current_key(wparam).filter(|ev| self.wants(ev)) else {
            return Ok(false.into());
        };
        let Some(context) = context.as_ref() else {
            return Ok(false.into());
        };
        let response = self.engine.borrow_mut().key(ev);
        let handled = response.handled;
        // Synchronous so a commit lands in the document before TSF hands an
        // un-eaten Space/Enter on to the app.
        self.apply(context, response, true);
        Ok(handled.into())
    }

    fn OnKeyUp(&self, _context: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> windows::core::Result<BOOL> {
        Ok(false.into())
    }

    fn OnPreservedKey(&self, _context: Ref<ITfContext>, _guid: *const GUID) -> windows::core::Result<BOOL> {
        Ok(false.into())
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _: Ref<ITfDocumentMgr>) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _: Ref<ITfDocumentMgr>) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnSetFocus(&self, _focus: Ref<ITfDocumentMgr>, _previous: Ref<ITfDocumentMgr>) -> windows::core::Result<()> {
        self.flush();
        Ok(())
    }

    fn OnPushContext(&self, _: Ref<ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _: Ref<ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    /// The app ended our composition (click elsewhere, its own commit). The
    /// preedit text already equals what the engine would commit, so it stays in
    /// the document; only our state and the underline go.
    fn OnCompositionTerminated(&self, ec: u32, composition: Ref<ITfComposition>) -> windows::core::Result<()> {
        let _ = self.engine.borrow_mut().reset();
        self.composing.set(false);
        let context = self.session.borrow().composition_context.clone();
        self.set_composition(None, None);
        self.hide_candidates();
        if let (Some(composition), Some(context)) = (composition.as_ref(), context) {
            let cleared = unsafe {
                composition
                    .GetRange()
                    .and_then(|range| context.GetProperty(&GUID_PROP_ATTRIBUTE).and_then(|p| p.Clear(ec, &range)))
            };
            if let Err(e) = cleared {
                log(&format!("OnCompositionTerminated: clearing attribute: {e}"));
            }
        }
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        Ok(display_attribute::enumerate())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> windows::core::Result<ITfDisplayAttributeInfo> {
        let guid = unsafe { guid.as_ref() }.ok_or(windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
        display_attribute::info(guid)
    }
}
