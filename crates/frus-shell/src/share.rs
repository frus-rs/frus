// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Sharing through the system's share sheet** (milestone 775): text, a link and files, handed
//! to whichever application the person picks — a message, a mail, a note, a nearby device.
//!
//! ```no_run
//! # fn f() -> Result<(), frus_shell::ShareError> {
//! // The common case: some text.
//! frus_shell::share("Meet at 6, the usual place")?;
//!
//! // Everything a share can carry.
//! frus_shell::Share::text("Here is the report")
//!     .subject("Weekly report")
//!     .url("https://example.com/reports/42")
//!     .file("/path/to/report.pdf")
//!     .show()?;
//! # Ok(())
//! # }
//! ```
//!
//! What each platform does:
//!
//! - **Android**: the system's chooser over an `ACTION_SEND` (`ACTION_SEND_MULTIPLE` for several
//!   files). Another application cannot read this one's files, so each file is first **copied to
//!   the shared Downloads collection**, in a folder named after the application, and the copy is
//!   what is shared; sharing the same name again replaces the earlier copy. That collection
//!   needs no permission from Android 10; on older versions a share with files is
//!   [`ShareError::Unsupported`], and text and links still go.
//! - **Windows**: the system's share window, for the application's window. Where it cannot be
//!   shown (no window of the application, an older Windows), the text and the link are **copied
//!   to the clipboard** instead and the answer is [`ShareOutcome::CopiedToClipboard`], for the
//!   application to say so.
//! - **macOS**: the system's share menu, over the application's window; the clipboard as on
//!   Windows when there is no window to show it on.
//! - **The web**: the browser's `navigator.share`, where it has one (most phones, some desktop
//!   browsers, and only on a secure page). Files have no path on the web, so a share with files
//!   is [`ShareError::Unsupported`] there, as is a browser without the share sheet.
//! - **Linux** and **iOS**: [`ShareError::Unsupported`].
//!
//! The sheet is shown and the call returns; what the person then picks is not reported.

use std::fmt;
use std::path::{Path, PathBuf};

/// Something to share through the system's share sheet: text, a subject, a link and files, any
/// of which may be left out — but not all of them.
///
/// ```no_run
/// # fn f() -> Result<(), frus_shell::ShareError> {
/// use frus_shell::{Share, ShareOutcome};
/// let shared = Share::new()
///     .url("https://frus.dev")
///     .subject("Have a look")
///     .show()?;
/// if shared == ShareOutcome::CopiedToClipboard {
///     // No share sheet here: tell the person the link is on the clipboard.
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Share {
    text: Option<String>,
    subject: Option<String>,
    url: Option<String>,
    files: Vec<PathBuf>,
}

/// What [`Share::show`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareOutcome {
    /// The system's share sheet is up.
    SheetShown,
    /// There is no share sheet here, so the text and the link were copied to the clipboard; the
    /// application may say so ("Copied").
    CopiedToClipboard,
}

/// Why [`Share::show`] did not share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareError {
    /// Nothing to share: no text, no link and no file.
    Empty,
    /// A file to share does not exist.
    FileNotFound(PathBuf),
    /// This platform, or this browser, has no share sheet for what was given.
    Unsupported,
    /// The system refused, for the reason given.
    Failed(String),
}

impl fmt::Display for ShareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShareError::Empty => f.write_str("nothing to share"),
            ShareError::FileNotFound(path) => write!(f, "no file at {}", path.display()),
            ShareError::Unsupported => f.write_str("no share sheet here"),
            ShareError::Failed(why) => write!(f, "the share sheet failed: {why}"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Shares `text` through the system's share sheet: [`Share::text`]`(text).show()`.
///
/// ```no_run
/// # fn f() -> Result<(), frus_shell::ShareError> {
/// frus_shell::share("I scored 42 points!")?;
/// # Ok(())
/// # }
/// ```
pub fn share(text: impl Into<String>) -> Result<ShareOutcome, ShareError> {
    Share::text(text).show()
}

impl Share {
    /// Nothing yet: add a [`url`](Self::url), [`file`](Self::file) or
    /// [`subject`](Self::subject).
    ///
    /// ```
    /// let share = frus_shell::Share::new().url("https://frus.dev");
    /// # let _ = share;
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Text to share — the body of a message or a mail.
    ///
    /// ```
    /// let share = frus_shell::Share::text("See you at 6").subject("Tonight");
    /// # let _ = share;
    /// ```
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    /// The subject, for an application that has one: a mail's subject line, the title of the
    /// share window on Windows.
    ///
    /// ```
    /// let share = frus_shell::Share::text("The numbers").subject("Weekly report");
    /// # let _ = share;
    /// ```
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// A link to share. Where the target takes only text, it follows the text on a line of
    /// its own.
    ///
    /// ```
    /// let share = frus_shell::Share::text("Have a look").url("https://frus.dev");
    /// # let _ = share;
    /// ```
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// A file to share; call it again for several.
    ///
    /// ```
    /// let share = frus_shell::Share::new().file("photo.jpg").file("notes.txt");
    /// # let _ = share;
    /// ```
    pub fn file(mut self, path: impl AsRef<Path>) -> Self {
        self.files.push(path.as_ref().to_path_buf());
        self
    }

    /// Shows the system's share sheet with what this share carries, and returns once it is up.
    ///
    /// ```no_run
    /// # fn f() -> Result<(), frus_shell::ShareError> {
    /// frus_shell::Share::text("Hello").subject("Greetings").show()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn show(&self) -> Result<ShareOutcome, ShareError> {
        if self.is_empty() {
            return Err(ShareError::Empty);
        }
        #[cfg(not(web))]
        if let Some(missing) = self.files.iter().find(|path| !path.is_file()) {
            return Err(ShareError::FileNotFound(missing.clone()));
        }
        platform::show(self)
    }

    /// Whether there is nothing to share.
    pub(crate) fn is_empty(&self) -> bool {
        let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
        blank(&self.text) && blank(&self.url) && self.files.is_empty()
    }

    /// The text and the link as one text, for a target that takes text alone: the link on a
    /// line of its own, unless the text already holds it.
    #[cfg_attr(
        not(any(windows, target_os = "macos", android, test)),
        allow(dead_code)
    )]
    pub(crate) fn plain_text(&self) -> Option<String> {
        let text = self.text.as_deref().filter(|t| !t.trim().is_empty());
        let url = self.url.as_deref().filter(|u| !u.trim().is_empty());
        match (text, url) {
            (Some(text), Some(url)) if text.contains(url) => Some(text.to_string()),
            (Some(text), Some(url)) => Some(format!("{text}\n{url}")),
            (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
            (None, None) => None,
        }
    }
}

/// The clipboard, for a desktop with no share sheet to show: what the share says in text.
#[cfg(any(windows, target_os = "macos"))]
fn copy_instead(share: &Share) -> Result<ShareOutcome, ShareError> {
    let Some(text) = share.plain_text() else {
        // Files alone have nothing to put on the clipboard as text.
        return Err(ShareError::Unsupported);
    };
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| ShareError::Failed(error.to_string()))?;
    log::info!("no share sheet could be shown: the text was copied to the clipboard");
    Ok(ShareOutcome::CopiedToClipboard)
}

#[cfg(windows)]
mod platform {
    //! The share window of Windows 10 and later, reached from a desktop application through
    //! `IDataTransferManagerInterop` for its window.

    use std::cell::RefCell;

    use windows::core::{factory, Interface, Ref, HSTRING};
    use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
    use windows::Foundation::{TypedEventHandler, Uri};
    use windows::Storage::{IStorageItem, StorageFile};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::IDataTransferManagerInterop;
    use windows_collections::IIterable;

    use super::{copy_instead, Share, ShareError, ShareOutcome};

    thread_local! {
        /// The window's manager and the handler last given to it: one share at a time, each
        /// replacing the one before.
        static REGISTERED: RefCell<Option<(DataTransferManager, i64)>> = const { RefCell::new(None) };
    }

    pub(super) fn show(share: &Share) -> Result<ShareOutcome, ShareError> {
        match show_sheet(share) {
            Ok(()) => Ok(ShareOutcome::SheetShown),
            Err(why) => {
                log::warn!("the share window could not be shown ({why})");
                copy_instead(share)
            }
        }
    }

    fn show_sheet(share: &Share) -> Result<(), String> {
        let hwnd = own_window().ok_or("the application has no window")?;
        let hwnd = HWND(hwnd);
        let interop = factory::<DataTransferManager, IDataTransferManagerInterop>()
            .map_err(|e| e.to_string())?;
        // SAFETY: `hwnd` is a top-level window of this process.
        let manager: DataTransferManager =
            unsafe { interop.GetForWindow(hwnd) }.map_err(|e| e.to_string())?;

        // The files, found when the window asks for them: WinRT's file objects do not cross
        // threads in these bindings, and the window asks on this one.
        let files = share.files.clone();
        // The window shows a title, and refuses a share without one.
        let title = share
            .subject
            .clone()
            .or_else(program_name)
            .unwrap_or_else(|| "Share".to_string());
        let text = share.text.clone().filter(|t| !t.trim().is_empty());
        let url = share.url.clone().filter(|u| !u.trim().is_empty());

        let handler = TypedEventHandler::new(
            move |_: Ref<DataTransferManager>, args: Ref<DataRequestedEventArgs>| {
                let data = args.ok()?.Request()?.Data()?;
                data.Properties()?
                    .SetTitle(&HSTRING::from(title.as_str()))?;
                if let Some(text) = &text {
                    data.SetText(&HSTRING::from(text.as_str()))?;
                }
                if let Some(url) = &url {
                    match Uri::CreateUri(&HSTRING::from(url.as_str())) {
                        Ok(uri) => data.SetWebLink(&uri)?,
                        // Not a link Windows reads: it goes as text.
                        Err(_) if text.is_none() => data.SetText(&HSTRING::from(url.as_str()))?,
                        Err(_) => {}
                    }
                }
                if !files.is_empty() {
                    let mut items: Vec<Option<IStorageItem>> = Vec::new();
                    for path in &files {
                        let file =
                            StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_path()))?
                                .join()?;
                        items.push(Some(file.cast::<IStorageItem>()?));
                    }
                    let items: IIterable<IStorageItem> = items.into();
                    data.SetStorageItems(&items, true)?;
                }
                Ok(())
            },
        );
        REGISTERED.with(|registered| -> Result<(), String> {
            let mut registered = registered.borrow_mut();
            if let Some((previous, token)) = registered.take() {
                let _ = previous.RemoveDataRequested(token);
            }
            let token = manager.DataRequested(&handler).map_err(|e| e.to_string())?;
            *registered = Some((manager, token));
            Ok(())
        })?;
        // SAFETY: as above.
        unsafe { interop.ShowShareUIForWindow(hwnd) }.map_err(|e| e.to_string())
    }

    /// The program's name, for the window's title when the share has no subject.
    fn program_name() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        Some(exe.file_stem()?.to_string_lossy().into_owned())
    }

    /// The application's window: the one in front if it is this process's, else its first
    /// visible top-level window.
    fn own_window() -> Option<*mut core::ffi::c_void> {
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
            GW_OWNER,
        };

        unsafe fn ours(hwnd: HWND) -> bool {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            pid == GetCurrentProcessId()
                && IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
        }
        unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> i32 {
            if ours(hwnd) {
                *(found as *mut HWND) = hwnd;
                return 0;
            }
            1
        }
        // SAFETY: plain Win32 queries; `found` outlives the enumeration that writes it.
        unsafe {
            let front = GetForegroundWindow();
            if !front.is_null() && ours(front) {
                return Some(front);
            }
            let mut found: HWND = std::ptr::null_mut();
            EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM);
            (!found.is_null()).then_some(found)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    //! `NSSharingServicePicker`, shown over the key window's content.

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{class, msg_send, MainThreadMarker};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

    use super::{copy_instead, Share, ShareError, ShareOutcome};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    pub(super) fn show(share: &Share) -> Result<ShareOutcome, ShareError> {
        // AppKit is the main thread's; an application's handlers run there.
        if MainThreadMarker::new().is_none() {
            log::warn!("the share menu is shown from the main thread only");
            return copy_instead(share);
        }
        match show_picker(share) {
            Ok(()) => Ok(ShareOutcome::SheetShown),
            Err(why) => {
                log::warn!("the share menu could not be shown ({why})");
                copy_instead(share)
            }
        }
    }

    fn show_picker(share: &Share) -> Result<(), &'static str> {
        let mut items: Vec<Retained<AnyObject>> = Vec::new();
        if let Some(text) = share.text.as_deref().filter(|t| !t.trim().is_empty()) {
            items.push(Retained::into_super(Retained::into_super(
                NSString::from_str(text),
            )));
        }
        if let Some(url) = share.url.as_deref().filter(|u| !u.trim().is_empty()) {
            match NSURL::URLWithString(&NSString::from_str(url)) {
                Some(link) => items.push(Retained::into_super(Retained::into_super(link))),
                None => items.push(Retained::into_super(Retained::into_super(
                    NSString::from_str(url),
                ))),
            }
        }
        for path in &share.files {
            let path = NSString::from_str(&path.to_string_lossy());
            let file = NSURL::fileURLWithPath(&path);
            items.push(Retained::into_super(Retained::into_super(file)));
        }
        let items = NSArray::from_retained_slice(&items);

        let app: Option<&AnyClass> = AnyClass::get(c"NSApplication");
        let app = app.ok_or("no NSApplication")?;
        // SAFETY: plain AppKit messages, on the main thread (checked by the caller).
        unsafe {
            let shared: *mut AnyObject = msg_send![app, sharedApplication];
            let shared = shared.as_ref().ok_or("no application")?;
            let mut window: *mut AnyObject = msg_send![shared, keyWindow];
            if window.is_null() {
                window = msg_send![shared, mainWindow];
            }
            let window = window.as_ref().ok_or("no window")?;
            let view: *mut AnyObject = msg_send![window, contentView];
            let view = view.as_ref().ok_or("no view")?;
            let bounds: NSRect = msg_send![view, bounds];
            // A point at the top middle of the window, which the menu hangs from.
            let anchor = NSRect::new(
                NSPoint::new(bounds.size.width / 2.0, bounds.size.height - 1.0),
                NSSize::new(1.0, 1.0),
            );
            let picker: *mut AnyObject = msg_send![class!(NSSharingServicePicker), alloc];
            let picker: *mut AnyObject = msg_send![picker, initWithItems: &*items];
            let picker = picker.as_ref().ok_or("no share menu")?;
            let _: () = msg_send![
                picker,
                showRelativeToRect: anchor,
                ofView: view,
                preferredEdge: NSRectEdge::MinY
            ];
            // The menu holds itself while it is up; this reference is the one `init` gave.
            let _: () = msg_send![picker, autorelease];
        }
        Ok(())
    }
}

#[cfg(android)]
mod platform {
    use super::{Share, ShareError, ShareOutcome};

    pub(super) fn show(share: &Share) -> Result<ShareOutcome, ShareError> {
        crate::android_share::share(
            share.plain_text().as_deref(),
            share.subject.as_deref(),
            &share.files,
        )
        .map(|()| ShareOutcome::SheetShown)
    }
}

#[cfg(web)]
mod platform {
    //! `navigator.share`, where the browser has it.

    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    use super::{Share, ShareError, ShareOutcome};

    pub(super) fn show(share: &Share) -> Result<ShareOutcome, ShareError> {
        if !share.files.is_empty() {
            // A path names nothing on the web.
            return Err(ShareError::Unsupported);
        }
        let navigator = web_sys::window()
            .ok_or(ShareError::Unsupported)?
            .navigator();
        let method = Reflect::get(&navigator, &"share".into())
            .ok()
            .and_then(|method| method.dyn_into::<Function>().ok())
            // No share sheet in this browser, or not on a page served over plain `http`.
            .ok_or(ShareError::Unsupported)?;
        let data = Object::new();
        let set = |key: &str, value: Option<&String>| {
            if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
                let _ = Reflect::set(&data, &key.into(), &JsValue::from_str(value));
            }
        };
        set("title", share.subject.as_ref());
        set("text", share.text.as_ref());
        set("url", share.url.as_ref());
        let promise = method
            .call1(&navigator, &data)
            .map_err(|error| ShareError::Failed(format!("{error:?}")))?;
        if let Ok(promise) = promise.dyn_into::<Promise>() {
            // The person closing the sheet rejects it too: worth a line, not an error.
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    log::debug!("the share sheet closed without sharing ({error:?})");
                }
            });
        }
        Ok(ShareOutcome::SheetShown)
    }
}

#[cfg(any(ios, all(desktop, not(any(windows, target_os = "macos")))))]
mod platform {
    use super::{Share, ShareError, ShareOutcome};

    pub(super) fn show(_share: &Share) -> Result<ShareOutcome, ShareError> {
        Err(ShareError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_keeps_what_it_is_given() {
        let share = Share::text("Here")
            .subject("Report")
            .url("https://example.com")
            .file("a.pdf")
            .file("b.png");
        assert_eq!(share.text.as_deref(), Some("Here"));
        assert_eq!(share.subject.as_deref(), Some("Report"));
        assert_eq!(share.url.as_deref(), Some("https://example.com"));
        assert_eq!(
            share.files,
            vec![PathBuf::from("a.pdf"), PathBuf::from("b.png")]
        );
    }

    #[test]
    fn nothing_to_share_is_an_error_not_an_empty_sheet() {
        assert_eq!(Share::new().show(), Err(ShareError::Empty));
        assert_eq!(Share::text("   ").show(), Err(ShareError::Empty));
        assert_eq!(
            Share::new().subject("only a subject").show(),
            Err(ShareError::Empty),
            "a subject alone says nothing"
        );
        assert!(!Share::new().url("https://frus.dev").is_empty());
        assert!(!Share::new().file("a.txt").is_empty());
    }

    #[test]
    fn a_missing_file_is_named() {
        let missing = std::env::temp_dir().join("frus-share-test-no-such-file.txt");
        assert_eq!(
            Share::text("x").file(&missing).show(),
            Err(ShareError::FileNotFound(missing))
        );
    }

    #[test]
    fn the_link_follows_the_text_on_its_own_line() {
        assert_eq!(
            Share::text("Look").url("https://frus.dev").plain_text(),
            Some("Look\nhttps://frus.dev".to_string())
        );
        assert_eq!(
            Share::new().url("https://frus.dev").plain_text(),
            Some("https://frus.dev".to_string())
        );
        assert_eq!(Share::text("Look").plain_text(), Some("Look".to_string()));
        assert_eq!(Share::new().file("a.txt").plain_text(), None);
    }

    #[test]
    fn a_link_the_text_already_holds_is_not_repeated() {
        assert_eq!(
            Share::text("See https://frus.dev today")
                .url("https://frus.dev")
                .plain_text(),
            Some("See https://frus.dev today".to_string())
        );
    }

    #[test]
    fn the_errors_say_what_happened() {
        assert_eq!(ShareError::Empty.to_string(), "nothing to share");
        assert_eq!(ShareError::Unsupported.to_string(), "no share sheet here");
    }
}
