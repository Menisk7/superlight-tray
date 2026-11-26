//! "Set DPI" window with a slider (native Win32 trackbar, no dependencies).
//!
//! The big label follows the slider while dragging. The DPI is applied to the
//! mouse when the slider is released (or moved with keys / mouse wheel), so the
//! user can feel it right away. OK keeps it; Cancel / Esc / close puts back the
//! DPI that was active when the window opened.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HFONT, HGDIOBJ};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_OK: usize = IDOK as usize;
const ID_CANCEL: usize = IDCANCEL as usize;
// Defined here to avoid pulling in more windows-sys modules.
const COLOR_BTNFACE: i32 = 15;
const TBM_GETPOS: u32 = WM_USER;
const TBM_SETPOS: u32 = WM_USER + 5;
const TBM_SETRANGE: u32 = WM_USER + 6;
const TBM_SETTIC: u32 = WM_USER + 4;
const TBM_SETPAGESIZE: u32 = WM_USER + 21;
const TB_THUMBTRACK: usize = 5;

struct Ctx {
    values: Vec<u16>,
    original: u16,
    applied: u16,
    label: HWND,
    slider: HWND,
    on_change: Box<dyn Fn(u16)>,
    result: Option<Option<u16>>,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    unsafe { SetWindowTextW(h, w.as_ptr()) };
}

fn current_value(c: &Ctx) -> u16 {
    let pos = unsafe { SendMessageW(c.slider, TBM_GETPOS, 0, 0) }.max(0) as usize;
    c.values[pos.min(c.values.len() - 1)]
}

/// Apply `dpi` if it differs from what was last applied.
fn apply(c: &mut Ctx, dpi: u16) {
    if c.applied != dpi {
        c.applied = dpi;
        (c.on_change)(dpi);
    }
}

fn finish(hwnd: HWND, ok: bool) {
    CTX.with(|cx| {
        if let Some(c) = cx.borrow_mut().as_mut() {
            if ok {
                let v = current_value(c);
                apply(c, v);
                c.result = Some(Some(v));
            } else {
                let orig = c.original;
                apply(c, orig);
                c.result = Some(None);
            }
        }
    });
    unsafe { DestroyWindow(hwnd) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HSCROLL => {
            CTX.with(|cx| {
                if let Some(c) = cx.borrow_mut().as_mut() {
                    let v = current_value(c);
                    set_text(c.label, &format!("{v} DPI"));
                    // apply on release / keyboard / wheel, not on every drag pixel
                    if wp & 0xFFFF != TB_THUMBTRACK {
                        apply(c, v);
                    }
                }
            });
            0
        }
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            if id == ID_OK || id == ID_CANCEL {
                finish(hwnd, id == ID_OK);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            finish(hwnd, false);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Show the slider window and block until OK / Cancel.
/// `on_change` is called with each DPI to apply live (and the original on cancel).
/// Returns the chosen DPI on OK.
pub fn pick_dpi(values: Vec<u16>, current: u16, ticks: &[u16], on_change: impl Fn(u16) + 'static) -> Option<u16> {
    if values.is_empty() {
        return None;
    }
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("SuperlightTrayDpi");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: LoadIconW(hinst, 1 as _),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_BTNFACE + 1) as usize as _,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc); // fails harmlessly if already registered

        // Layout in 96-DPI units, scaled to the system DPI.
        let dpi = GetDpiForSystem().max(96) as i32;
        let px = |v: i32| v * dpi / 96;
        let (cw, ch) = (px(460), px(196));
        let style = WS_CAPTION | WS_SYSMENU | WS_POPUP;
        let ex = WS_EX_TOPMOST | WS_EX_DLGMODALFRAME;
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: cw,
            bottom: ch,
        };
        AdjustWindowRectEx(&mut rc, style, 0, ex);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;

        let title = wide(crate::ui::SET_DPI_TITLE);
        let hwnd = CreateWindowExW(
            ex,
            class.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return None;
        }

        let font = message_font(1, 400);
        let big = message_font(2, 600);
        let child = |cls: &str, text: &str, st: u32, x: i32, y: i32, w: i32, h: i32, id: usize, f: HFONT| {
            let (c, t) = (wide(cls), wide(text));
            let c = CreateWindowExW(
                0,
                c.as_ptr(),
                t.as_ptr(),
                WS_CHILD | WS_VISIBLE | st,
                px(x),
                px(y),
                px(w),
                px(h),
                hwnd,
                id as _,
                hinst,
                std::ptr::null(),
            );
            if !f.is_null() {
                SendMessageW(c, WM_SETFONT, f as WPARAM, 1);
            }
            c
        };
        const SS_CENTER: u32 = 0x1;
        const SS_RIGHT: u32 = 0x2;
        let label = child("STATIC", &format!("{current} DPI"), SS_CENTER, 14, 12, 432, 40, 0, big);
        let slider = child(
            "msctls_trackbar32",
            "",
            WS_TABSTOP,
            14,
            60,
            432,
            36,
            0,
            std::ptr::null_mut(),
        );
        child("STATIC", &format!("{}", values[0]), 0, 20, 100, 100, 20, 0, font);
        child(
            "STATIC",
            &format!("{}", values[values.len() - 1]),
            SS_RIGHT,
            340,
            100,
            100,
            20,
            0,
            font,
        );
        child(
            "BUTTON",
            "OK",
            WS_TABSTOP | (BS_DEFPUSHBUTTON as u32),
            270,
            150,
            84,
            30,
            ID_OK,
            font,
        );
        child(
            "BUTTON",
            "Cancel",
            WS_TABSTOP | (BS_PUSHBUTTON as u32),
            362,
            150,
            84,
            30,
            ID_CANCEL,
            font,
        );

        let max = (values.len() - 1) as isize;
        SendMessageW(slider, TBM_SETRANGE, 1, max << 16);
        SendMessageW(slider, TBM_SETPAGESIZE, 0, 8);
        for t in ticks {
            if let Some(i) = values.iter().position(|v| v == t) {
                SendMessageW(slider, TBM_SETTIC, 0, i as isize);
            }
        }
        let start = crate::ui::slider_index(&values, current);
        SendMessageW(slider, TBM_SETPOS, 1, start as isize);

        CTX.with(|cx| {
            *cx.borrow_mut() = Some(Ctx {
                values,
                original: current,
                applied: current,
                label,
                slider,
                on_change: Box::new(on_change),
                result: None,
            })
        });

        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(slider);

        // Local modal loop. IsDialogMessage gives Enter/Esc/Tab/arrow handling.
        let mut msg: MSG = std::mem::zeroed();
        while CTX.with(|cx| cx.borrow().as_ref().is_some_and(|c| c.result.is_none())) {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r == 0 {
                PostQuitMessage(msg.wParam as i32);
                break;
            }
            if r < 0 {
                break;
            }
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for f in [font, big] {
            if !f.is_null() {
                DeleteObject(f as HGDIOBJ);
            }
        }
        CTX.with(|cx| cx.borrow_mut().take()).and_then(|c| c.result).flatten()
    }
}

/// The system message-box font (Segoe UI), scaled by `mult`, with `weight`.
unsafe fn message_font(mult: i32, weight: i32) -> HFONT {
    let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
    ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
    if SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut _, 0) == 0 {
        return std::ptr::null_mut();
    }
    let mut lf = ncm.lfMessageFont;
    lf.lfHeight *= mult;
    lf.lfWeight = weight;
    CreateFontIndirectW(&lf)
}
