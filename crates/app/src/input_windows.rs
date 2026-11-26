//! A small native "type a number" window (Win32, no dependencies).
//! Enter = OK, Esc = Cancel, Tab moves between controls.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HFONT, HGDIOBJ};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_EDIT: usize = 100;
// Defined here to avoid pulling in more windows-sys modules.
const COLOR_BTNFACE: i32 = 15;
const EM_SETSEL: u32 = 0x00B1;
const EM_LIMITTEXT: u32 = 0x00C5;
const ID_OK: usize = IDOK as usize;
const ID_CANCEL: usize = IDCANCEL as usize;

thread_local! {
    static RESULT: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    static EDIT: RefCell<HWND> = const { RefCell::new(std::ptr::null_mut()) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            if id == ID_OK {
                let edit = EDIT.with(|e| *e.borrow());
                let mut buf = [0u16; 32];
                let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
                let text = String::from_utf16_lossy(&buf[..n]);
                RESULT.with(|r| *r.borrow_mut() = Some(Some(text)));
                DestroyWindow(hwnd);
                return 0;
            }
            if id == ID_CANCEL {
                RESULT.with(|r| *r.borrow_mut() = Some(None));
                DestroyWindow(hwnd);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            RESULT.with(|r| *r.borrow_mut() = Some(None));
            DestroyWindow(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Show the window and block until OK/Cancel. Returns the typed text on OK.
pub fn input_box(title: &str, prompt: &str, initial: &str) -> Option<String> {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("SuperlightTrayInput");
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
        let (cw, ch) = (px(340), px(132));
        let style = WS_CAPTION | WS_SYSMENU | WS_POPUP;
        let ex = WS_EX_TOPMOST | WS_EX_DLGMODALFRAME;
        let mut rc = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: cw,
            bottom: ch,
        };
        AdjustWindowRectEx(&mut rc, style, 0, ex);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;

        let title_w = wide(title);
        let hwnd = CreateWindowExW(
            ex,
            class.as_ptr(),
            title_w.as_ptr(),
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

        let font = message_font();
        let child = |cls: &str, text: &str, st: u32, x: i32, y: i32, w: i32, h: i32, id: usize, ex: u32| {
            let (c, t) = (wide(cls), wide(text));
            let c = CreateWindowExW(
                ex,
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
            if !font.is_null() {
                SendMessageW(c, WM_SETFONT, font as WPARAM, 1);
            }
            c
        };
        child("STATIC", prompt, 0, 14, 14, 312, 22, 0, 0);
        let edit = child(
            "EDIT",
            initial,
            WS_TABSTOP | (ES_NUMBER as u32) | (ES_AUTOHSCROLL as u32),
            14,
            44,
            150,
            26,
            ID_EDIT,
            WS_EX_CLIENTEDGE,
        );
        child(
            "BUTTON",
            "OK",
            WS_TABSTOP | (BS_DEFPUSHBUTTON as u32),
            150,
            90,
            84,
            28,
            ID_OK,
            0,
        );
        child(
            "BUTTON",
            "Cancel",
            WS_TABSTOP | (BS_PUSHBUTTON as u32),
            242,
            90,
            84,
            28,
            ID_CANCEL,
            0,
        );
        SendMessageW(edit, EM_LIMITTEXT, 6, 0);
        SendMessageW(edit, EM_SETSEL, 0, -1);
        EDIT.with(|e| *e.borrow_mut() = edit);
        RESULT.with(|r| *r.borrow_mut() = None);

        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(edit);

        // Local modal loop. IsDialogMessage gives Enter/Esc/Tab handling.
        let mut msg: MSG = std::mem::zeroed();
        while RESULT.with(|r| r.borrow().is_none()) {
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
        if !font.is_null() {
            DeleteObject(font as HGDIOBJ);
        }
        RESULT.with(|r| r.borrow_mut().take()).flatten()
    }
}

/// The font Windows uses for message boxes (Segoe UI), at the system DPI.
unsafe fn message_font() -> HFONT {
    let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
    ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
    if SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut _, 0) == 0 {
        return std::ptr::null_mut();
    }
    CreateFontIndirectW(&ncm.lfMessageFont)
}
