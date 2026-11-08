// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Watching the device's networks** (milestone 777): the platform half of
//! [`frus_widgets::connectivity`]. Each platform reports what the system says to
//! [`frus_widgets::report_connectivity`], from whichever thread the system calls back on; a
//! change asks for a frame, and the frame builds the interface again.
//!
//! The watch starts on the first read, not before: an application that never asks pays
//! nothing.

/// Tells the widget layer how to start watching. Called once, as the shell starts.
pub(crate) fn install() {
    frus_widgets::install_connectivity_watch(start);
}

/// Starts watching; `false` when it cannot yet (Android's bridge not loaded) and must be asked
/// again.
fn start() -> bool {
    #[cfg(android)]
    {
        crate::android_device::start_connectivity()
    }
    #[cfg(web)]
    {
        crate::web_device::start_connectivity();
        true
    }
    #[cfg(windows)]
    {
        windows_watch::start();
        true
    }
    #[cfg(target_os = "macos")]
    {
        apple::start();
        true
    }
    #[cfg(all(desktop, not(any(windows, target_os = "macos"))))]
    {
        linux::start();
        true
    }
    #[cfg(ios)]
    {
        // Not watched yet: the value stays `Unknown`.
        true
    }
}

/// What the web says: `navigator.onLine`, and the connection's type as
/// `navigator.connection.type` names it where the browser has it.
#[cfg(any(web, test))]
pub(crate) fn of_web_type(online: bool, kind: Option<&str>) -> frus_widgets::Connectivity {
    if !online {
        return frus_widgets::Connectivity::None;
    }
    match kind {
        Some("wifi") => frus_widgets::Connectivity::Wifi,
        Some("cellular") => frus_widgets::Connectivity::Cellular,
        Some("ethernet") => frus_widgets::Connectivity::Ethernet,
        Some("none") => frus_widgets::Connectivity::None,
        _ => frus_widgets::Connectivity::Other,
    }
}

/// Windows: the connection profile Windows uses for the internet, read again on each of its
/// network events.
#[cfg(windows)]
mod windows_watch {
    use frus_widgets::{report_connectivity, Connectivity};
    use windows::Networking::Connectivity::{
        NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
    };

    pub(super) fn start() {
        let now = read();
        log::debug!("connectivity: {now:?}");
        report_connectivity(now);
        let handler = NetworkStatusChangedEventHandler::new(|_| {
            let now = read();
            if report_connectivity(now) {
                log::debug!("connectivity: {now:?}");
            }
            Ok(())
        });
        // Kept for the life of the process: the token is never revoked.
        if let Err(err) = NetworkInformation::NetworkStatusChanged(&handler) {
            log::warn!("connectivity changes will not be seen ({err})");
        }
    }

    /// What the internet's profile says. No profile is no network.
    pub(super) fn read() -> Connectivity {
        let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
            return Connectivity::None;
        };
        match profile.GetNetworkConnectivityLevel() {
            Ok(level) if level == NetworkConnectivityLevel::None => return Connectivity::None,
            Ok(_) => {}
            Err(_) => return Connectivity::Unknown,
        }
        if profile.IsWlanConnectionProfile().unwrap_or(false) {
            return Connectivity::Wifi;
        }
        if profile.IsWwanConnectionProfile().unwrap_or(false) {
            return Connectivity::Cellular;
        }
        match profile
            .NetworkAdapter()
            .and_then(|adapter| adapter.IanaInterfaceType())
        {
            Ok(kind) => of_iana_type(kind),
            Err(_) => Connectivity::Other,
        }
    }

    /// An adapter's IANA interface type: 6 is ethernet, 71 is 802.11, 243 and 244 are mobile
    /// broadband.
    pub(super) fn of_iana_type(kind: u32) -> Connectivity {
        match kind {
            6 => Connectivity::Ethernet,
            71 => Connectivity::Wifi,
            243 | 244 => Connectivity::Cellular,
            _ => Connectivity::Other,
        }
    }
}

/// macOS: the path the system would route traffic through, as `NWPathMonitor` reports it.
#[cfg(target_os = "macos")]
mod apple {
    use frus_widgets::{report_connectivity, Connectivity};
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};

    type Object = *mut c_void;

    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_path_monitor_create() -> Object;
        fn nw_path_monitor_set_queue(monitor: Object, queue: Object);
        fn nw_path_monitor_set_update_handler(
            monitor: Object,
            handler: &block2::Block<dyn Fn(Object)>,
        );
        fn nw_path_monitor_start(monitor: Object);
        fn nw_path_get_status(path: Object) -> i32;
        fn nw_path_uses_interface_type(path: Object, kind: i32) -> bool;
    }

    extern "C" {
        fn dispatch_queue_create(label: *const c_char, attributes: Object) -> Object;
    }

    /// Whether the monitor has reported once.
    static HEARD: AtomicBool = AtomicBool::new(false);

    pub(super) fn start() {
        let handler = block2::RcBlock::new(|path: Object| {
            // SAFETY: the monitor hands a valid path for the length of the call.
            let now = unsafe { classify(path) };
            if report_connectivity(now) {
                log::debug!("connectivity: {now:?}");
            }
            HEARD.store(true, Ordering::Release);
        });
        // SAFETY: plain calls into the Network framework; the monitor and the queue live for
        // the life of the process, and the monitor copies the block.
        unsafe {
            let queue =
                dispatch_queue_create(c"dev.frus.connectivity".as_ptr(), std::ptr::null_mut());
            let monitor = nw_path_monitor_create();
            if monitor.is_null() || queue.is_null() {
                log::warn!("connectivity unknown: no path monitor");
                return;
            }
            nw_path_monitor_set_queue(monitor, queue);
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
        }
        // The first report comes at once, on the monitor's queue: waited for a moment, so that
        // the first read already has it.
        for _ in 0..30 {
            if HEARD.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// `nw_path_status_satisfied` is 1; the interface types are other 0, Wi-Fi 1, cellular 2,
    /// wired 3.
    unsafe fn classify(path: Object) -> Connectivity {
        if path.is_null() || nw_path_get_status(path) != 1 {
            return Connectivity::None;
        }
        if nw_path_uses_interface_type(path, 1) {
            Connectivity::Wifi
        } else if nw_path_uses_interface_type(path, 2) {
            Connectivity::Cellular
        } else if nw_path_uses_interface_type(path, 3) {
            Connectivity::Ethernet
        } else {
            Connectivity::Other
        }
    }
}

/// Linux: the interface of the default route, and what kind it is, looked at every two seconds.
/// Reading two small files of the kernel's needs no D-Bus service and works on every
/// distribution, with or without NetworkManager.
#[cfg(any(test, all(desktop, not(any(windows, target_os = "macos")))))]
#[cfg_attr(test, allow(dead_code))]
mod linux {
    use frus_widgets::{report_connectivity, Connectivity};
    use std::path::Path;

    /// How often the routes are looked at.
    const EVERY: std::time::Duration = std::time::Duration::from_secs(2);

    pub(super) fn start() {
        let now = read();
        log::debug!("connectivity: {now:?}");
        report_connectivity(now);
        let spawned = std::thread::Builder::new()
            .name("frus-connectivity".into())
            .spawn(|| loop {
                std::thread::sleep(EVERY);
                let now = read();
                if report_connectivity(now) {
                    log::debug!("connectivity: {now:?}");
                }
            });
        if let Err(err) = spawned {
            log::warn!("connectivity changes will not be seen ({err})");
        }
    }

    pub(super) fn read() -> Connectivity {
        let v4 = std::fs::read_to_string("/proc/net/route").ok();
        let v6 = std::fs::read_to_string("/proc/net/ipv6_route").ok();
        if v4.is_none() && v6.is_none() {
            return Connectivity::Unknown;
        }
        let name = v4
            .as_deref()
            .and_then(default_v4)
            .or_else(|| v6.as_deref().and_then(default_v6));
        let Some(name) = name else {
            return Connectivity::None;
        };
        let dir = Path::new("/sys/class/net").join(&name);
        classify(&Interface {
            name: &name,
            uevent: &std::fs::read_to_string(dir.join("uevent")).unwrap_or_default(),
            wireless: dir.join("wireless").exists() || dir.join("phy80211").exists(),
            arp_type: std::fs::read_to_string(dir.join("type"))
                .ok()
                .and_then(|t| t.trim().parse().ok()),
        })
    }

    /// The interface of the first default route that is up, in `/proc/net/route`.
    pub(super) fn default_v4(table: &str) -> Option<String> {
        table.lines().skip(1).find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (name, destination, flags, mask) = (
                fields.first()?,
                fields.get(1)?,
                fields.get(3)?,
                fields.get(7)?,
            );
            let up = u32::from_str_radix(flags, 16).ok()? & 1 != 0;
            (*destination == "00000000" && *mask == "00000000" && up && *name != "lo")
                .then(|| name.to_string())
        })
    }

    /// The interface of the first default route that is up, in `/proc/net/ipv6_route`.
    pub(super) fn default_v6(table: &str) -> Option<String> {
        table.lines().find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                return None;
            }
            let up = u32::from_str_radix(fields[8], 16).ok()? & 1 != 0;
            let default = fields[0].bytes().all(|b| b == b'0') && fields[1] == "00";
            (default && up && fields[9] != "lo").then(|| fields[9].to_string())
        })
    }

    /// What the kernel says of an interface.
    pub(super) struct Interface<'a> {
        pub name: &'a str,
        /// Its `uevent`, which names a `DEVTYPE` for Wi-Fi and mobile broadband.
        pub uevent: &'a str,
        /// Whether it has a `wireless` or `phy80211` directory.
        pub wireless: bool,
        /// Its ARP hardware type: 1 is ethernet.
        pub arp_type: Option<u32>,
    }

    pub(super) fn classify(interface: &Interface) -> Connectivity {
        let devtype = interface
            .uevent
            .lines()
            .find_map(|line| line.strip_prefix("DEVTYPE="));
        if interface.wireless || devtype == Some("wlan") {
            Connectivity::Wifi
        } else if devtype == Some("wwan")
            || ["wwan", "rmnet", "ppp"]
                .iter()
                .any(|prefix| interface.name.starts_with(prefix))
        {
            Connectivity::Cellular
        } else if interface.arp_type == Some(1) && devtype.is_none() {
            Connectivity::Ethernet
        } else {
            Connectivity::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::linux::*;
    use frus_widgets::Connectivity;

    const ROUTE: &str =
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0
wlp2s0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
wlp2s0\t0000A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
";

    #[test]
    fn the_default_route_names_the_way_out() {
        assert_eq!(default_v4(ROUTE).as_deref(), Some("wlp2s0"));
        // Only the local networks: no way out.
        let local: String = ROUTE
            .lines()
            .filter(|l| !l.contains("0100A8C0"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(default_v4(&local), None);
    }

    #[test]
    fn an_ipv6_only_default_route_counts() {
        let table = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 enp3s0\n\
                     00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
        assert_eq!(default_v6(table).as_deref(), Some("enp3s0"));
    }

    #[test]
    fn an_interface_is_told_by_what_the_kernel_says_of_it() {
        let of = |name, uevent, wireless, arp_type| {
            classify(&Interface {
                name,
                uevent,
                wireless,
                arp_type,
            })
        };
        assert_eq!(
            of("wlp2s0", "DEVTYPE=wlan\nINTERFACE=wlp2s0", true, Some(1)),
            Connectivity::Wifi
        );
        assert_eq!(
            of("eth0", "INTERFACE=eth0\nIFINDEX=2", false, Some(1)),
            Connectivity::Ethernet
        );
        assert_eq!(
            of("wwan0", "DEVTYPE=wwan", false, Some(1)),
            Connectivity::Cellular
        );
        assert_eq!(
            of("tun0", "INTERFACE=tun0", false, Some(65534)),
            Connectivity::Other
        );
        // A bridge is ethernet to ARP, and a bridge to the kernel.
        assert_eq!(
            of("br0", "DEVTYPE=bridge", false, Some(1)),
            Connectivity::Other
        );
    }

    /// **The machine's own answer**, through the public read: run by hand with `--ignored
    /// --nocapture`. With `FRUS_WATCH_SECS` set it keeps printing each change for that long, to
    /// see a cable pulled or Wi-Fi turned off.
    #[test]
    #[ignore = "reads this machine's networks"]
    fn reads_this_machine() {
        super::install();
        let first = frus_widgets::connectivity();
        println!("connectivity now: {first:?}");
        let secs: u64 = std::env::var("FRUS_WATCH_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        while std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if frus_widgets::take_connectivity_change() {
                println!("connectivity changed: {:?}", frus_widgets::connectivity());
            }
        }
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        assert_ne!(first, Connectivity::Unknown);
    }

    #[test]
    fn the_web_says_online_and_sometimes_how() {
        use super::of_web_type;
        assert_eq!(of_web_type(false, Some("wifi")), Connectivity::None);
        assert_eq!(of_web_type(true, None), Connectivity::Other);
        assert_eq!(of_web_type(true, Some("cellular")), Connectivity::Cellular);
        assert_eq!(of_web_type(true, Some("wifi")), Connectivity::Wifi);
        assert_eq!(of_web_type(true, Some("unknown")), Connectivity::Other);
    }

    #[cfg(windows)]
    #[test]
    fn an_adapters_type_names_its_kind() {
        use super::windows_watch::of_iana_type;
        assert_eq!(of_iana_type(6), Connectivity::Ethernet);
        assert_eq!(of_iana_type(71), Connectivity::Wifi);
        assert_eq!(of_iana_type(243), Connectivity::Cellular);
        assert_eq!(of_iana_type(131), Connectivity::Other);
    }
}
