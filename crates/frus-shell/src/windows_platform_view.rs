// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Windows host of **native views** (milestone 802): each view lives in a child window
//! of frus's window — its *container* — which frus moves, resizes and clips with the frame.
//!
//! A child window is drawn by the system **above** frus's swap chain, whatever frus draws: the
//! classic airspace problem. So frus cuts out of the container, with a window region, the
//! parts it draws over the view (a menu, a button laid on it), and hides the container while
//! the view is out of sight. While a dialog covers the view whole, the container is hidden and
//! a picture of it (`PrintWindow` with `PW_RENDERFULLCONTENT`, which renders DirectComposition
//! content too) is drawn by frus under the dialog: the scrim dims it like any frus content.
//!
//! Input needs nothing: the system sends the mouse and the keyboard to the window under the
//! pointer and to the focused one. A view taking the focus says so (`frus.focus`), and a press
//! on frus gives the focus back to frus's window.
//!
//! Two view types are built in: `frus/edittext`, a system `EDIT` control, and `frus/webview`,
//! WebView2 (feature `webview2`, `webview2.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CombineRgn, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateRectRgn,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, FrameRect, GetDC, ReleaseDC,
    SelectObject, SetWindowRgn, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HFONT,
    HGDIOBJ, PAINTSTRUCT, RGN_OR,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_RETURN};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, IsChild, LoadCursorW, RegisterClassExW, SendMessageW,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW, EN_CHANGE,
    EN_KILLFOCUS, EN_SETFOCUS, ES_AUTOHSCROLL, GWLP_USERDATA, IDC_ARROW, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNA, WM_CHAR, WM_COMMAND, WM_ERASEBKGND, WM_KEYDOWN, WM_NCDESTROY,
    WM_PAINT, WM_SETFONT, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_TABSTOP,
    WS_VISIBLE,
};

use crate::platform_view::{PixelRect, Shape};
use crate::view_host::{self, NativeView, PlatformViewChannel, PlatformViewContext, ViewHost};
use frus_widgets::PlatformViewEvent;

#[cfg(feature = "webview2")]
#[path = "webview2.rs"]
mod webview2;

/// The view types this host makes with no plugin.
pub(crate) fn register_builtins() {
    frus_widgets::register_platform_view_type("frus/edittext");
    #[cfg(feature = "webview2")]
    frus_widgets::register_platform_view_type("frus/webview");
}

/// What fills a container.
enum Content {
    /// A system `EDIT` control: `frus/edittext`.
    Edit(HWND),
    /// A WebView2: `frus/webview`.
    #[cfg(feature = "webview2")]
    Web(webview2::Web),
    /// A plugin's view.
    Plugin(Box<dyn NativeView>),
    /// Nothing: the view could not be made (its widget was told).
    Nothing,
}

struct Hosted {
    /// The container.
    hwnd: HWND,
    content: Content,
    at: PixelRect,
    shown: bool,
    /// The web view controller whose page this is (milestone 804): kept while its widget is
    /// away, and handed to the next widget of the same controller.
    page: Option<u64>,
}

/// The views of one window.
pub(crate) struct Host {
    parent: HWND,
    views: HashMap<i32, Hosted>,
    /// Controllers' pages whose widget went, hidden until one comes back or the controller
    /// is dropped (`web.release`).
    parked: HashMap<u64, Hosted>,
    /// Physical pixels per logical one, for the built-in controls' font and margins.
    scale: f32,
    /// The built-in field's font, at `scale`.
    font: HFONT,
}

/// The container window's class name, NUL-terminated UTF-16.
fn class_name() -> Vec<u16> {
    wide("FrusPlatformViewHost")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Bit 0 of the container's user data: paint a field's white box and frame behind the view.
const PAINT_FIELD: isize = 1;

impl Host {
    /// The host of the views of `parent`, frus's window; `None` when the container's window
    /// class cannot be registered.
    pub(crate) fn new(window: &Arc<winit::window::Window>, parent: HWND) -> Option<Self> {
        // SAFETY: plain Win32 calls with valid arguments; the class is registered once (a
        // second registration fails harmlessly with "class already exists").
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let name = class_name();
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(container_proc),
                cbClsExtra: 0,
                // One pointer-sized slot: what the container paints (`PAINT_FIELD`).
                cbWndExtra: std::mem::size_of::<isize>() as i32,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&class);
        }
        let wake = window.clone();
        view_host::set_wake(Arc::new(move || wake.request_redraw()));
        Some(Self {
            parent,
            views: HashMap::new(),
            parked: HashMap::new(),
            scale: 0.0,
            font: std::ptr::null_mut(),
        })
    }

    /// The window's scale: the built-in controls follow it.
    pub(crate) fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        // SAFETY: the old font is no longer selected anywhere once every field has the new
        // one; the font handle is ours.
        unsafe {
            let old = self.font;
            self.font = CreateFontW(
                -((15.0 * scale).round() as i32),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1, // DEFAULT_CHARSET
                0,
                0,
                5, // CLEARTYPE_QUALITY
                0,
                wide("Segoe UI").as_ptr(),
            );
            for hosted in self.views.values() {
                if let Content::Edit(edit) = hosted.content {
                    SendMessageW(edit, WM_SETFONT, self.font as WPARAM, 1);
                    lay_out_field(edit, hosted.at.w, hosted.at.h, scale);
                }
            }
            if !old.is_null() {
                DeleteObject(old as HGDIOBJ);
            }
        }
    }

    /// Whether the keyboard focus is in one of the views (or anywhere inside frus's window
    /// but frus's own): the window is still the one being used.
    pub(crate) fn focus_inside(parent: HWND) -> bool {
        // SAFETY: plain queries.
        unsafe {
            let focus = GetFocus();
            !focus.is_null() && focus != parent && IsChild(parent, focus) != 0
        }
    }

    fn make_content(
        &mut self,
        view: i32,
        view_type: &str,
        params: &str,
        container: HWND,
    ) -> Content {
        let (w, h) = self.views.get(&view).map_or((0, 0), |v| (v.at.w, v.at.h));
        if let Some(factory) = view_host::factory(view_type) {
            let context = PlatformViewContext {
                params,
                channel: PlatformViewChannel::new(view),
                width: w,
                height: h,
                parent: container as isize,
            };
            return match factory(&context) {
                Ok(made) => Content::Plugin(made),
                Err(why) => {
                    view_host::send(view, PlatformViewEvent::ERROR, &why);
                    Content::Nothing
                }
            };
        }
        match view_type {
            "frus/edittext" => self.make_field(view, params, container, w, h),
            #[cfg(feature = "webview2")]
            "frus/webview" => Content::Web(webview2::Web::new(container, view, params, w, h)),
            other => {
                view_host::send(
                    view,
                    PlatformViewEvent::ERROR,
                    &format!("no native view type \"{other}\" on Windows"),
                );
                Content::Nothing
            }
        }
    }

    fn make_field(&mut self, view: i32, hint: &str, container: HWND, w: i32, h: i32) -> Content {
        // SAFETY: the container is alive; the class is the system's.
        unsafe {
            SetWindowLongPtrW(container, 0, PAINT_FIELD);
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                0,
                1,
                1,
                container,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if edit.is_null() {
                view_host::send(
                    view,
                    PlatformViewEvent::ERROR,
                    "the EDIT control could not be made",
                );
                return Content::Nothing;
            }
            if !self.font.is_null() {
                SendMessageW(edit, WM_SETFONT, self.font as WPARAM, 0);
            }
            set_hint(edit, hint);
            SetWindowSubclass(edit, Some(field_proc), 1, view as usize);
            lay_out_field(edit, w, h, self.scale);
            Content::Edit(edit)
        }
    }

    /// Drops a view: the view first (a plugin's window, the web view's controller), then its
    /// container.
    fn destroy(hosted: Hosted) {
        drop(hosted.content);
        // SAFETY: the container is ours; destroying it destroys what is left inside.
        unsafe {
            DestroyWindow(hosted.hwnd);
        }
    }

    /// Gives view `view` the page of controller `page` if another view shows it or it is
    /// parked (milestone 804): its container, moved to `at`. Says whether it did.
    #[cfg(feature = "webview2")]
    fn adopt(&mut self, view: i32, page: u64, at: PixelRect) -> bool {
        let shown_by = self
            .views
            .iter()
            .find(|(_, hosted)| hosted.page == Some(page))
            .map(|(v, _)| *v);
        let hosted = match shown_by {
            Some(other) => self.views.remove(&other),
            None => self.parked.remove(&page),
        };
        let Some(mut hosted) = hosted else {
            return false;
        };
        if let Content::Web(web) = &mut hosted.content {
            web.set_view(view);
        }
        // SAFETY: the container is ours and alive.
        unsafe { SetWindowLongPtrW(hosted.hwnd, GWLP_USERDATA, view as isize) };
        self.views.insert(view, hosted);
        // Hidden until shaped, as a new view is; the other view's commands now find nothing.
        self.shape(view, &Shape::Hidden);
        self.place(view, at);
        true
    }

    /// A call for every web view (milestone 804): a page let go, the cookies.
    #[cfg(feature = "webview2")]
    pub(crate) fn web_global(&mut self, method: &str, arguments: &str) {
        use crate::web_view_protocol::Global;
        let Some(global) = Global::parse(method, arguments) else {
            return;
        };
        match global {
            Global::Release { controller } => {
                if let Some(hosted) = self.parked.remove(&controller) {
                    Self::destroy(hosted);
                }
                // Still shown (its widget goes this frame): destroyed when it goes.
                for hosted in self.views.values_mut() {
                    if hosted.page == Some(controller) {
                        hosted.page = None;
                    }
                }
            }
            cookie_call => {
                let webs = self
                    .views
                    .values()
                    .chain(self.parked.values())
                    .filter_map(|hosted| match &hosted.content {
                        Content::Web(web) => Some(web),
                        _ => None,
                    });
                webview2::cookies(cookie_call, webs);
            }
        }
    }

    fn resize_content(&mut self, view: i32) {
        let scale = self.scale;
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let (w, h) = (hosted.at.w, hosted.at.h);
        match &mut hosted.content {
            Content::Edit(edit) => lay_out_field(*edit, w, h, scale),
            #[cfg(feature = "webview2")]
            Content::Web(web) => web.resized(w, h),
            Content::Plugin(plugin) => plugin.resized(w, h),
            Content::Nothing => {}
        }
    }
}

impl ViewHost for Host {
    fn create(&mut self, view: i32, view_type: &str, params: &str, at: PixelRect) {
        #[cfg(feature = "webview2")]
        if view_type == "frus/webview" && view_host::factory(view_type).is_none() {
            let page = crate::web_view_protocol::Params::parse(params).map(|p| p.controller);
            if let Some(page) = page {
                if self.adopt(view, page, at) {
                    return;
                }
            }
        }
        // SAFETY: the parent is frus's live window; the class was registered in `new`.
        let container = unsafe {
            CreateWindowExW(
                0,
                class_name().as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                at.x,
                at.y,
                at.w.max(1),
                at.h.max(1),
                self.parent,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if container.is_null() {
            view_host::send(
                view,
                PlatformViewEvent::ERROR,
                "the container window could not be made",
            );
            return;
        }
        // SAFETY: the container is ours.
        unsafe { SetWindowLongPtrW(container, GWLP_USERDATA, view as isize) };
        self.views.insert(
            view,
            Hosted {
                hwnd: container,
                content: Content::Nothing,
                at,
                shown: false,
                page: None,
            },
        );
        let content = self.make_content(view, view_type, params, container);
        #[cfg(feature = "webview2")]
        let page = match &content {
            Content::Web(_) => {
                crate::web_view_protocol::Params::parse(params).map(|p| p.controller)
            }
            _ => None,
        };
        if let Some(hosted) = self.views.get_mut(&view) {
            hosted.content = content;
            #[cfg(feature = "webview2")]
            {
                hosted.page = page;
            }
        }
    }

    fn place(&mut self, view: i32, at: PixelRect) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let resized = (hosted.at.w, hosted.at.h) != (at.w, at.h);
        hosted.at = at;
        // SAFETY: the container is ours and alive.
        unsafe {
            SetWindowPos(
                hosted.hwnd,
                std::ptr::null_mut(),
                at.x,
                at.y,
                at.w.max(1),
                at.h.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        if resized {
            self.resize_content(view);
        }
    }

    fn shape(&mut self, view: i32, shape: &Shape) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let hwnd = hosted.hwnd;
        // SAFETY: the container is ours; a region handed to the system is the system's.
        unsafe {
            match shape {
                Shape::Whole => {
                    SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
                }
                Shape::Parts(parts) => {
                    let region = CreateRectRgn(0, 0, 0, 0);
                    for part in parts {
                        let piece = CreateRectRgn(part.x, part.y, part.x + part.w, part.y + part.h);
                        CombineRgn(region, region, piece, RGN_OR);
                        DeleteObject(piece as HGDIOBJ);
                    }
                    SetWindowRgn(hwnd, region, 1);
                }
                Shape::Hidden | Shape::Covered => {}
            }
            let show = shape.shows();
            if show != hosted.shown {
                hosted.shown = show;
                ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
                match &mut hosted.content {
                    #[cfg(feature = "webview2")]
                    Content::Web(web) => web.set_visible(show),
                    Content::Plugin(plugin) => plugin.visibility_changed(show),
                    _ => {}
                }
            }
        }
    }

    fn dispose(&mut self, view: i32) {
        let Some(mut hosted) = self.views.remove(&view) else {
            return;
        };
        // A controller's page waits, hidden, for its controller's next widget.
        if let Some(page) = hosted.page {
            if hosted.shown {
                hosted.shown = false;
                // SAFETY: the container is ours and alive.
                unsafe {
                    ShowWindow(hosted.hwnd, SW_HIDE);
                }
            }
            #[cfg(feature = "webview2")]
            if let Content::Web(web) = &mut hosted.content {
                web.set_visible(false);
            }
            if let Some(old) = self.parked.insert(page, hosted) {
                Self::destroy(old);
            }
            return;
        }
        Self::destroy(hosted);
    }

    fn invoke(&mut self, view: i32, method: &str, arguments: &str) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        match &mut hosted.content {
            Content::Edit(edit) => match method {
                // SAFETY: the control is ours and alive.
                "setText" => unsafe {
                    SetWindowTextW(*edit, wide(arguments).as_ptr());
                },
                "setHint" => set_hint(*edit, arguments),
                _ => {}
            },
            #[cfg(feature = "webview2")]
            Content::Web(web) => web.invoke(method, arguments),
            Content::Plugin(plugin) => plugin.invoke(method, arguments),
            Content::Nothing => {}
        }
    }

    fn snapshot(&mut self, view: i32) -> Option<frus_widgets::ImageHandle> {
        let hosted = self.views.get(&view)?;
        if !hosted.shown {
            return None;
        }
        // SAFETY: a memory DC and a DIB section of our own, released before returning.
        unsafe { picture(hosted.hwnd, hosted.at.w, hosted.at.h) }
    }

    fn clear_focus(&mut self) {
        if Self::focus_inside(self.parent) {
            // SAFETY: frus's window is alive and ours.
            unsafe {
                SetFocus(self.parent);
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let parked = self.parked.drain().map(|(_, hosted)| hosted);
        for hosted in self.views.drain().map(|(_, hosted)| hosted).chain(parked) {
            Self::destroy(hosted);
        }
        if !self.font.is_null() {
            // SAFETY: every field that used it is gone.
            unsafe {
                DeleteObject(self.font as HGDIOBJ);
            }
        }
    }
}

/// A picture of `hwnd` and what is inside, `w` × `h`, as straight RGBA.
unsafe fn picture(hwnd: HWND, w: i32, h: i32) -> Option<frus_widgets::ImageHandle> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let screen = GetDC(std::ptr::null_mut());
    let memory = CreateCompatibleDC(screen);
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        // Negative: the rows top to bottom.
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(
        memory,
        &info,
        DIB_RGB_COLORS,
        &mut bits,
        std::ptr::null_mut(),
        0,
    );
    let mut image = None;
    if !bitmap.is_null() && !bits.is_null() {
        let old = SelectObject(memory, bitmap as HGDIOBJ);
        // PW_RENDERFULLCONTENT (2): what DirectComposition draws too — a web view's page.
        if PrintWindow(hwnd, memory, 2) != 0 {
            let len = (w * h * 4) as usize;
            let bgra = std::slice::from_raw_parts(bits as *const u8, len);
            let mut rgba = Vec::with_capacity(len);
            for pixel in bgra.as_chunks::<4>().0 {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
            }
            image =
                Some(frus_widgets::ImageData::from_rgba(w as u32, h as u32, rgba).into_handle());
        }
        SelectObject(memory, old);
        DeleteObject(bitmap as HGDIOBJ);
    }
    DeleteDC(memory);
    ReleaseDC(std::ptr::null_mut(), screen);
    image
}

/// The field inside its container: the line of text centred, a margin on each side.
fn lay_out_field(edit: HWND, w: i32, h: i32, scale: f32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let margin = (10.0 * scale).round() as i32;
    let line = ((15.0 * 1.45) * scale).round() as i32;
    let line = line.min(h.max(1));
    // SAFETY: the control is alive.
    unsafe {
        SetWindowPos(
            edit,
            std::ptr::null_mut(),
            margin.min(w / 2),
            ((h - line) / 2).max(0),
            (w - 2 * margin).max(1),
            line.max(1),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// The grey hint shown in an empty field.
fn set_hint(edit: HWND, hint: &str) {
    // EM_SETCUEBANNER, with the hint shown even while the field has the focus.
    const EM_SETCUEBANNER: u32 = 0x1501;
    let text = wide(hint);
    // SAFETY: the control is alive; it copies the text.
    unsafe {
        SendMessageW(edit, EM_SETCUEBANNER, 1, text.as_ptr() as LPARAM);
    }
}

fn text_of(hwnd: HWND) -> String {
    // SAFETY: the buffer is as long as the system says the text is, plus its NUL.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0u16; len.max(0) as usize + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..read.max(0) as usize])
    }
}

/// The container's window procedure: a field's notifications become its widget's events.
unsafe extern "system" fn container_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let view = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as i32;
            let code = ((wparam >> 16) & 0xffff) as u32;
            let control = lparam as HWND;
            match code {
                EN_CHANGE => view_host::send(view, "changed", &text_of(control)),
                EN_SETFOCUS => view_host::send(view, PlatformViewEvent::FOCUS, "true"),
                EN_KILLFOCUS => view_host::send(view, PlatformViewEvent::FOCUS, "false"),
                _ => {}
            }
            0
        }
        // The content covers the container: nothing to erase, and no flash.
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            if GetWindowLongPtrW(hwnd, 0) & PAINT_FIELD != 0 {
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                let white = CreateSolidBrush(0x00ff_ffff);
                FillRect(dc, &rect, white);
                DeleteObject(white as HGDIOBJ);
                let frame = CreateSolidBrush(0x0076_7676);
                FrameRect(dc, &rect, frame);
                DeleteObject(frame as HGDIOBJ);
            }
            EndPaint(hwnd, &paint);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The field's own procedure, before the control's: Enter submits.
unsafe extern "system" fn field_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    view: usize,
) -> LRESULT {
    match message {
        WM_KEYDOWN if wparam == VK_RETURN as WPARAM => {
            view_host::send(view as i32, "submitted", &text_of(hwnd));
            0
        }
        // The control would beep at the Enter it cannot type.
        WM_CHAR if wparam == 0x0d => 0,
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(field_proc), 1);
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}
