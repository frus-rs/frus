// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Which network the device is on** (milestone 777): [`connectivity`], and
//! [`BuildContext::use_connectivity`](crate::BuildContext::use_connectivity) for a component
//! that shows it.
//!
//! The value lives here, where the widgets can read it; the platform layer watches the system
//! and writes it. It is shared between threads because the systems report from threads of their
//! own (Android's network callbacks, Windows' network events, the polling thread on Linux).
//!
//! **Connected is not online.** A Wi-Fi network with a captive portal, a router whose line is
//! down, a firewall: the device is on a network and still reaches nothing. A request is the only
//! proof that a server answers; this says which way out the device has, if any.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;

/// Which network the device uses to reach the outside, as the system reports it.
///
/// **On a network is not on the internet**: see [`connectivity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Connectivity {
    /// No network at all: airplane mode, no Wi-Fi in range, a cable unplugged.
    None,
    /// A Wi-Fi network.
    Wifi,
    /// The mobile network.
    Cellular,
    /// A cable.
    Ethernet,
    /// A network of another kind (a VPN, Bluetooth tethering, a USB link), or one the platform
    /// does not name — a browser that only says whether it is online.
    Other,
    /// frus cannot tell on this platform, or has not heard from the system yet.
    Unknown,
}

impl Connectivity {
    /// Whether the device is on some network. Not whether the internet answers: see
    /// [`connectivity`].
    ///
    /// ```
    /// use frus_widgets::Connectivity;
    /// assert!(Connectivity::Wifi.is_connected());
    /// assert!(!Connectivity::None.is_connected());
    /// assert!(!Connectivity::Unknown.is_connected());
    /// ```
    pub fn is_connected(self) -> bool {
        !matches!(self, Connectivity::None | Connectivity::Unknown)
    }

    fn code(self) -> u8 {
        match self {
            Connectivity::None => 0,
            Connectivity::Wifi => 1,
            Connectivity::Cellular => 2,
            Connectivity::Ethernet => 3,
            Connectivity::Other => 4,
            Connectivity::Unknown => 5,
        }
    }

    fn from_code(code: u8) -> Connectivity {
        match code {
            0 => Connectivity::None,
            1 => Connectivity::Wifi,
            2 => Connectivity::Cellular,
            3 => Connectivity::Ethernet,
            4 => Connectivity::Other,
            _ => Connectivity::Unknown,
        }
    }
}

/// What the system said last.
static CURRENT: AtomicU8 = AtomicU8::new(5);
/// Whether it changed since the platform layer last looked.
static CHANGED: AtomicBool = AtomicBool::new(false);
/// What starts watching the system: installed by the platform layer, run on the first read.
static WATCH: OnceLock<fn() -> bool> = OnceLock::new();
/// Whether the watch has started.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// **Which network the device is on**, as the system says now, and frus keeps up to date.
///
/// The first call starts watching the system, which then reports every change; a component
/// that shows it rebuilds on each one (the whole interface is built again, and reads the new
/// value). Outside a running application — a unit test — it is [`Connectivity::Unknown`].
///
/// **Connected does not mean the internet is reachable.** A captive portal, a router with no
/// line behind it, a network that blocks the server: the device is on a network, and a request
/// still fails. Use this to explain a failure or to wait for a network before retrying, never
/// to decide that a request will work. What each platform reports:
///
/// - **Android**: the default network (`ConnectivityManager`'s callback). Needs
///   `android.permission.ACCESS_NETWORK_STATE`, which `frus build` and `frus run` declare.
/// - **Windows**: the connection profile Windows uses for the internet.
/// - **macOS**: the path the system would route traffic through (`NWPathMonitor`).
/// - **Linux**: the interface of the default route, looked at every two seconds.
/// - **The web**: `navigator.onLine`, and the connection's type where the browser tells it
///   (Chrome on Android); [`Connectivity::Other`] when it only says "online".
/// - **iOS**: [`Connectivity::Unknown`] for now.
///
/// ```
/// let net = frus_widgets::connectivity();
/// if !net.is_connected() {
///     // say "you are offline", and retry once it changes
/// }
/// ```
#[inline(never)]
pub fn connectivity() -> Connectivity {
    if !WATCHING.load(Ordering::Acquire) {
        if let Some(watch) = WATCH.get() {
            if watch() {
                WATCHING.store(true, Ordering::Release);
            }
        }
    }
    Connectivity::from_code(CURRENT.load(Ordering::Acquire))
}

/// Installs what starts watching the system's networks; it answers whether it started (a
/// bridge not loaded yet answers `false`, and is asked again on the next read). The platform
/// layer calls this once; an application never does.
#[doc(hidden)]
#[inline(never)]
pub fn install_connectivity_watch(watch: fn() -> bool) {
    let _ = WATCH.set(watch);
}

/// What the system says now: stored, and whether it differs from what it said before; a
/// change asks the shell for a frame, which builds the interface again. Any thread may call it. The platform layer calls this; an application never does.
#[doc(hidden)]
#[inline(never)]
pub fn report_connectivity(now: Connectivity) -> bool {
    let before = CURRENT.swap(now.code(), Ordering::AcqRel);
    let changed = before != now.code();
    if changed {
        CHANGED.store(true, Ordering::Release);
        crate::local_task::request_frame();
    }
    changed
}

/// Whether the connectivity changed since this was last called; forgets it. The platform
/// layer reads it once a frame, and builds the interface again when it did.
#[doc(hidden)]
#[inline(never)]
pub fn take_connectivity_change() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}

impl crate::BuildContext<'_> {
    /// **Which network the device is on**, for a component that shows it — an "offline"
    /// banner, a "waiting for Wi-Fi" note. The component is built again whenever it changes.
    /// See [`connectivity`] for what each platform reports, and why connected is not online.
    ///
    /// ```
    /// use frus_widgets::{text, BuildContext, Connectivity, Widget};
    ///
    /// fn status(cx: &BuildContext) -> Box<dyn Widget> {
    ///     Box::new(text(match cx.use_connectivity() {
    ///         Connectivity::None => "Offline",
    ///         Connectivity::Wifi => "On Wi-Fi",
    ///         Connectivity::Cellular => "On mobile data",
    ///         _ => "Online",
    ///     }))
    /// }
    /// ```
    pub fn use_connectivity(&self) -> Connectivity {
        // Every change rebuilds the whole interface (`take_connectivity_change`), so reading
        // the value is all a component needs: no slot, and it may be called conditionally.
        connectivity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_value_survives_its_code() {
        for c in [
            Connectivity::None,
            Connectivity::Wifi,
            Connectivity::Cellular,
            Connectivity::Ethernet,
            Connectivity::Other,
            Connectivity::Unknown,
        ] {
            assert_eq!(Connectivity::from_code(c.code()), c);
        }
    }

    /// A report that changes the value is a change, once; the same value again is none. The
    /// statics are process-wide, so this is the one test that writes them.
    #[test]
    fn a_change_is_reported_once_and_a_repeat_is_not() {
        assert!(report_connectivity(Connectivity::Wifi) || connectivity() == Connectivity::Wifi);
        let _ = take_connectivity_change();
        assert!(!report_connectivity(Connectivity::Wifi), "the same value");
        assert!(!take_connectivity_change());
        assert!(report_connectivity(Connectivity::None));
        assert_eq!(connectivity(), Connectivity::None);
        assert!(take_connectivity_change(), "seen once");
        assert!(!take_connectivity_change(), "and forgotten");
    }
}
