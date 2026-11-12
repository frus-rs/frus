// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus/webview` on Windows (milestone 802, feature `webview2`): the system's WebView2 in a
//! view's container.
//!
//! WebView2 is made asynchronously: the environment, then the controller, each answered on the
//! window's thread from its message loop — which winit runs. Until the controller exists, the
//! calls the application makes wait, in order, as do the size and the visibility.
//!
//! It speaks what Android's `frus/webview` speaks: the calls `loadUrl`, `loadHtml`,
//! `evaluateJavascript` (answered by `javascriptResult`, the value as JSON), `goBack`,
//! `goForward`, `reload` and `stop`; the events `pageStarted` and `pageFinished` (the
//! address), and `frus.focus`.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    ExecuteScriptCompletedHandler, FocusChangedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};

use crate::view_host;
use frus_widgets::PlatformViewEvent;

struct Ready {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

#[derive(Default)]
struct State {
    ready: Option<Ready>,
    /// Calls made before the controller existed, in order.
    pending: Vec<(String, String)>,
    size: (i32, i32),
    visible: bool,
    /// The widget left before the controller came: it is closed as it comes.
    gone: bool,
}

/// One web view.
pub(super) struct Web {
    view: i32,
    state: Rc<RefCell<State>>,
}

impl Web {
    /// A web view in `container`, `w` × `h`, first showing `params`: an address, or HTML when
    /// it starts with `<`.
    pub(super) fn new(
        container: windows_sys::Win32::Foundation::HWND,
        view: i32,
        params: &str,
        w: i32,
        h: i32,
    ) -> Self {
        let mut pending = Vec::new();
        let first = params.trim_start();
        if first.starts_with('<') {
            pending.push(("loadHtml".to_owned(), params.to_owned()));
        } else if !first.is_empty() {
            pending.push(("loadUrl".to_owned(), params.to_owned()));
        }
        let state = Rc::new(RefCell::new(State {
            pending,
            size: (w, h),
            visible: false,
            ..Default::default()
        }));
        let parent = HWND(container);
        let made = state.clone();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |result, environment: Option<ICoreWebView2Environment>| {
                let environment = match (result, environment) {
                    (Ok(()), Some(environment)) => environment,
                    (result, _) => {
                        failed(view, &format!("WebView2 could not start ({result:?})"));
                        return Ok(());
                    }
                };
                let made = made.clone();
                let controller_made = CreateCoreWebView2ControllerCompletedHandler::create(
                    Box::new(move |result, controller: Option<ICoreWebView2Controller>| {
                        match (result, controller) {
                            (Ok(()), Some(controller)) => ready(view, &made, controller),
                            (result, _) => failed(
                                view,
                                &format!("the WebView2 controller could not be made ({result:?})"),
                            ),
                        }
                        Ok(())
                    }),
                );
                // SAFETY: the container is alive (a gone view's is closed as it comes).
                if let Err(err) =
                    unsafe { environment.CreateCoreWebView2Controller(parent, &controller_made) }
                {
                    failed(
                        view,
                        &format!("the WebView2 controller could not be made ({err})"),
                    );
                }
                Ok(())
            },
        ));
        // The browser's data (cookies, storage, cache) in the application's own folder, not
        // beside the executable, which may not be writable.
        let folder = crate::data_dir::app_data_dir()
            .map(|dir| HSTRING::from(dir.join("WebView2").as_os_str()))
            .unwrap_or_default();
        let folder = if folder.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(folder.as_ptr())
        };
        // SAFETY: the strings outlive the call; the handler is reference-counted.
        let started = unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                folder,
                None::<&ICoreWebView2EnvironmentOptions>,
                &handler,
            )
        };
        if let Err(err) = started {
            failed(
                view,
                &format!(
                    "WebView2 is not available: is the Edge WebView2 runtime installed? ({err})"
                ),
            );
        }
        Self { view, state }
    }

    /// The container was resized.
    pub(super) fn resized(&mut self, w: i32, h: i32) {
        let mut state = self.state.borrow_mut();
        state.size = (w, h);
        if let Some(ready) = &state.ready {
            bounds(&ready.controller, w, h);
        }
    }

    /// The view came into sight, or went out of it: WebView2 stops drawing while hidden.
    pub(super) fn set_visible(&mut self, visible: bool) {
        let mut state = self.state.borrow_mut();
        state.visible = visible;
        if let Some(ready) = &state.ready {
            // SAFETY: a live controller.
            let _ = unsafe { ready.controller.SetIsVisible(visible) };
        }
    }

    /// The application called `method`.
    pub(super) fn invoke(&mut self, method: &str, arguments: &str) {
        let mut state = self.state.borrow_mut();
        match &state.ready {
            Some(ready) => call(self.view, &ready.webview, method, arguments),
            None => state
                .pending
                .push((method.to_owned(), arguments.to_owned())),
        }
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.gone = true;
        if let Some(ready) = state.ready.take() {
            // SAFETY: a live controller, closed once.
            let _ = unsafe { ready.controller.Close() };
        }
    }
}

fn failed(view: i32, why: &str) {
    log::warn!("native view {view}: {why}");
    view_host::send(view, PlatformViewEvent::ERROR, why);
}

fn bounds(controller: &ICoreWebView2Controller, w: i32, h: i32) {
    // SAFETY: a live controller.
    let _ = unsafe {
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: w.max(0),
            bottom: h.max(0),
        })
    };
}

/// The controller came: it takes the size, the visibility and the waiting calls.
fn ready(view: i32, state: &Rc<RefCell<State>>, controller: ICoreWebView2Controller) {
    let mut state = state.borrow_mut();
    if state.gone {
        // SAFETY: a live controller, closed once.
        let _ = unsafe { controller.Close() };
        return;
    }
    // SAFETY: a live controller and its web view; the handlers are reference-counted and
    // hold no reference back to them.
    let webview = match unsafe { controller.CoreWebView2() } {
        Ok(webview) => webview,
        Err(err) => {
            failed(view, &format!("the WebView2 has no page ({err})"));
            return;
        }
    };
    bounds(&controller, state.size.0, state.size.1);
    // SAFETY: as above.
    unsafe {
        let _ = controller.SetIsVisible(state.visible);
        let mut token = 0;
        let _ = webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut uri = windows::core::PWSTR::null();
                    if args.Uri(&mut uri).is_ok() {
                        view_host::send(view, "pageStarted", &webview2_com::take_pwstr(uri));
                    }
                }
                Ok(())
            })),
            &mut token,
        );
        let _ = webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, _| {
                let mut uri = windows::core::PWSTR::null();
                let url = match sender {
                    Some(webview) if webview.Source(&mut uri).is_ok() => {
                        webview2_com::take_pwstr(uri)
                    }
                    _ => String::new(),
                };
                view_host::send(view, "pageFinished", &url);
                Ok(())
            })),
            &mut token,
        );
        let _ = controller.add_GotFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                view_host::send(view, PlatformViewEvent::FOCUS, "true");
                Ok(())
            })),
            &mut token,
        );
        let _ = controller.add_LostFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                view_host::send(view, PlatformViewEvent::FOCUS, "false");
                Ok(())
            })),
            &mut token,
        );
    }
    for (method, arguments) in std::mem::take(&mut state.pending) {
        call(view, &webview, &method, &arguments);
    }
    state.ready = Some(Ready {
        controller,
        webview,
    });
}

/// Carries out `method` on `webview`.
fn call(view: i32, webview: &ICoreWebView2, method: &str, arguments: &str) {
    // SAFETY: a live web view; the strings outlive each call.
    let result = unsafe {
        match method {
            "loadUrl" => webview.Navigate(&HSTRING::from(arguments)),
            "loadHtml" => webview.NavigateToString(&HSTRING::from(arguments)),
            "evaluateJavascript" => webview.ExecuteScript(
                &HSTRING::from(arguments),
                &ExecuteScriptCompletedHandler::create(Box::new(move |result, value: String| {
                    match result {
                        Ok(()) => view_host::send(view, "javascriptResult", &value),
                        Err(err) => view_host::send(view, "javascriptResult", &format!("{err}")),
                    }
                    Ok(())
                })),
            ),
            "goBack" => webview.GoBack(),
            "goForward" => webview.GoForward(),
            "reload" => webview.Reload(),
            "stop" => webview.Stop(),
            _ => Ok(()),
        }
    };
    if let Err(err) = result {
        log::warn!("native view {view}: {method} failed ({err})");
    }
}
