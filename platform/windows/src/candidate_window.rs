//! Own candidate popup for the focused segment (D8 up/down). It is created on
//! the app's UI thread in Activate and never activates, so the app keeps focus.

use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, DeleteObject, EndPaint, FillRect, FrameRect, GetDC, GetDeviceCaps, GetSysColor, GetSysColorBrush,
    GetTextExtentPoint32W, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, COLOR_HIGHLIGHT,
    COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWFRAME, COLOR_WINDOWTEXT, DEFAULT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
    FW_NORMAL, HDC, HFONT, LOGPIXELSY, PAINTSTRUCT, TRANSPARENT, CLEARTYPE_QUALITY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    UnregisterClassW, GWLP_USERDATA, HWND_TOPMOST, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_MOUSEACTIVATE, WM_PAINT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use nullime_engine::Segment;

const CLASS: PCWSTR = w!("NullIMECandidateWindow");
const PAD: i32 = 4;

struct Model {
    lines: Vec<Vec<u16>>,
    selected: usize,
    line_height: i32,
    font: HFONT,
}

pub struct CandidateWindow {
    hwnd: HWND,
    // Boxed so the pointer stored in GWLP_USERDATA stays valid for the HWND's life.
    model: Box<RefCell<Model>>,
}

impl CandidateWindow {
    pub fn new() -> windows::core::Result<CandidateWindow> {
        let instance = HINSTANCE(crate::module().0);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        // Zero means "already registered" by another thread's instance; fine.
        unsafe { RegisterClassExW(&class) };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?
        };
        let font = unsafe {
            let dc = GetDC(Some(hwnd));
            let height = -(10 * GetDeviceCaps(Some(dc), LOGPIXELSY) / 72);
            ReleaseDC(Some(hwnd), dc);
            // Segoe UI font-links to Malgun Gothic / Yu Gothic for Hangul and kanji.
            CreateFontW(
                height,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            )
        };
        let model = Box::new(RefCell::new(Model { lines: Vec::new(), selected: 0, line_height: 0, font }));
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*model as *const _ as isize) };
        Ok(CandidateWindow { hwnd, model })
    }

    /// `anchor` is the focused segment's screen rect; the list opens below it.
    pub fn show(&self, segment: &Segment, anchor: RECT) {
        let (width, height) = {
            let mut m = self.model.borrow_mut();
            m.lines = segment
                .candidates
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{}. {}  [{:?}]", i + 1, c.text, c.lang).encode_utf16().collect())
                .collect();
            m.selected = segment.selected;
            let (w, h) = measure(self.hwnd, &m);
            m.line_height = h;
            (w + 2 * PAD, h * m.lines.len() as i32 + 2 * PAD)
        };
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), anchor.left, anchor.bottom, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(self.model.borrow().font.into());
            // The class points at this DLL's wndproc; unregister so a later reload
            // of the DLL at another address does not reuse a stale class. Fails
            // harmlessly while another thread still has a window of this class.
            let _ = UnregisterClassW(CLASS, Some(HINSTANCE(crate::module().0)));
        }
    }
}

/// Widest line and the line height, in pixels.
fn measure(hwnd: HWND, m: &Model) -> (i32, i32) {
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, m.font.into());
        let (mut width, mut height) = (0, 0);
        for line in m.lines.iter().chain(std::iter::once(&vec![b'M' as u16])) {
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, line, &mut size);
            width = width.max(size.cx);
            height = height.max(size.cy);
        }
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
        (width, height)
    }
}

fn paint(dc: HDC, client: RECT, m: &Model) {
    unsafe {
        FillRect(dc, &client, GetSysColorBrush(COLOR_WINDOW));
        FrameRect(dc, &client, GetSysColorBrush(COLOR_WINDOWFRAME));
        let old = SelectObject(dc, m.font.into());
        SetBkMode(dc, TRANSPARENT);
        for (i, line) in m.lines.iter().enumerate() {
            let top = PAD + i as i32 * m.line_height;
            if i == m.selected {
                let row = RECT { left: 1, top, right: client.right - 1, bottom: top + m.line_height };
                FillRect(dc, &row, GetSysColorBrush(COLOR_HIGHLIGHT));
                SetTextColor(dc, windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)));
            } else {
                SetTextColor(dc, windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
            }
            let _ = TextOutW(dc, PAD, top, line);
        }
        SelectObject(dc, old);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Clicking the list must not steal focus from the app's edit control.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            let model = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<Model>;
            if let Some(model) = unsafe { model.as_ref() } {
                if let Ok(m) = model.try_borrow() {
                    let mut client = RECT::default();
                    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client) };
                    paint(dc, client, &m);
                }
            }
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
