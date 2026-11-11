// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A disk cache for fetched images** that follows what the server said (milestone 795).
//!
//! An entry is one file in the application's cache folder, named after its address, holding
//! the body and what decides whether it may be shown without asking again:
//!
//! - `Cache-Control: no-store` is never written; `no-cache` (or `max-age=0`) is kept and
//!   asked about every time it is shown;
//! - `max-age` (less the `Age` the response had already) or else `Expires`, measured from the
//!   server's `Date`, says how long it is fresh;
//! - a response that says nothing is fresh for a tenth of the time since its `Last-Modified`
//!   (the usual heuristic, at most [`HEURISTIC_CAP`]), or for [`DEFAULT_FRESHNESS`] when it
//!   has no date either — what image caches commonly do;
//! - a stale entry is asked about with `If-None-Match` (its `ETag`) and `If-Modified-Since`
//!   (its `Last-Modified`): a `304` makes it fresh again without the body, and a network that
//!   is not there shows the stale copy rather than nothing.
//!
//! The folder is kept under a size ([`DEFAULT_DISK_CACHE_BYTES`] unless the application says
//! otherwise): past it, the entries used least recently — each file's modification time,
//! touched when it is read — are removed.
//!
//! The parsing and the rules are plain functions, tested on their own; the file I/O is a
//! thin layer over them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a response that says nothing about its freshness, and has no date to guess
/// from, is shown without asking again: seven days.
pub(crate) const DEFAULT_FRESHNESS: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The longest a freshness guessed from `Last-Modified` may be: seven days as well.
pub(crate) const HEURISTIC_CAP: Duration = DEFAULT_FRESHNESS;

/// The size the image cache's folder is kept under unless the application sets another:
/// 100 MiB.
pub const DEFAULT_DISK_CACHE_BYTES: u64 = 100 * 1024 * 1024;

/// What a response says about being kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Policy {
    /// Whether it may be written at all (`no-store` says not).
    pub store: bool,
    /// Until when, in seconds since 1970, it may be shown without asking.
    pub fresh_until: u64,
    /// Its `ETag`, to ask with `If-None-Match`.
    pub etag: Option<String>,
    /// Its `Last-Modified`, to ask with `If-Modified-Since`.
    pub last_modified: Option<String>,
}

/// The seconds since 1970 of `at`.
pub(crate) fn unix(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What `headers` say about keeping a response received at `now` (seconds since 1970).
pub(crate) fn policy(headers: &[(String, String)], now: u64) -> Policy {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_string())
    };
    let mut store = true;
    let mut no_cache = false;
    let mut max_age: Option<u64> = None;
    for control in headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("cache-control"))
    {
        for directive in control.1.split(',') {
            let directive = directive.trim();
            let (name, value) = match directive.split_once('=') {
                Some((n, v)) => (n.trim(), Some(v.trim().trim_matches('"'))),
                None => (directive, None),
            };
            match name.to_ascii_lowercase().as_str() {
                "no-store" => store = false,
                "no-cache" => no_cache = true,
                // A private cache reads `max-age` and leaves `s-maxage` to shared ones.
                "max-age" => max_age = value.and_then(|v| v.parse().ok()).or(Some(0)),
                _ => {}
            }
        }
    }
    let etag = header("etag").filter(|v| !v.is_empty());
    let last_modified = header("last-modified").filter(|v| !v.is_empty());
    let date = header("date").and_then(|d| parse_http_date(&d));
    let freshness = if no_cache {
        0
    } else if let Some(max_age) = max_age {
        let age = header("age")
            .and_then(|a| a.parse::<u64>().ok())
            .unwrap_or(0);
        max_age.saturating_sub(age)
    } else if let Some(expires) = header("expires") {
        // An `Expires` that cannot be read — `0`, `-1` — means already expired.
        match parse_http_date(&expires) {
            Some(expires) => expires.saturating_sub(date.unwrap_or(now)),
            None => 0,
        }
    } else if let Some(modified) = last_modified.as_deref().and_then(parse_http_date) {
        let since = date.unwrap_or(now).saturating_sub(modified);
        (since / 10).min(HEURISTIC_CAP.as_secs())
    } else {
        DEFAULT_FRESHNESS.as_secs()
    };
    Policy {
        store,
        fresh_until: now.saturating_add(freshness),
        etag,
        last_modified,
    }
}

/// Seconds since 1970 of an HTTP date: the preferred `Sun, 06 Nov 1994 08:49:37 GMT`, and
/// the two obsolete forms a server may still send (`Sunday, 06-Nov-94 08:49:37 GMT`, and
/// C's `Sun Nov  6 08:49:37 1994`).
pub(crate) fn parse_http_date(text: &str) -> Option<u64> {
    let words: Vec<&str> = text.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    let month = |name: &str| {
        [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ]
        .iter()
        .position(|m| name.eq_ignore_ascii_case(m))
        .map(|i| i as u64 + 1)
    };
    let (day, month, year, time) = match words.as_slice() {
        // IMF-fixdate: Sun, 06 Nov 1994 08:49:37 GMT
        [_, day, mon, year, time, ..] if mon.len() == 3 && year.len() == 4 => {
            (day.parse().ok()?, month(mon)?, year.parse().ok()?, *time)
        }
        // RFC 850: Sunday, 06-Nov-94 08:49:37 GMT
        [_, dmy, time, ..] if dmy.matches('-').count() == 2 => {
            let mut parts = dmy.split('-');
            let day = parts.next()?.parse().ok()?;
            let mon = month(parts.next()?)?;
            let year: u64 = parts.next()?.parse().ok()?;
            let year = if year < 70 {
                2000 + year
            } else if year < 100 {
                1900 + year
            } else {
                year
            };
            (day, mon, year, *time)
        }
        // asctime: Sun Nov  6 08:49:37 1994
        [_, mon, day, time, year] => (day.parse().ok()?, month(mon)?, year.parse().ok()?, *time),
        _ => return None,
    };
    let mut hms = time.split(':').map(|p| p.parse::<u64>().ok());
    let (h, m, s) = (hms.next()??, hms.next()??, hms.next()??);
    if !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 || year < 1970 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468).max(0) as u64
}

/// The name of the file kept for `url`: a 64-bit FNV-1a hash of it, in hexadecimal. Two
/// addresses that collide are told apart by the address written inside.
pub(crate) fn file_name(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}.img")
}

/// The first bytes of every entry, so that a file that is not one is never served.
const MAGIC: &[u8; 8] = b"FRUSIMG1";

/// One kept response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub url: String,
    pub policy: Policy,
    pub body: Vec<u8>,
}

impl Entry {
    /// Whether it may be shown at `now` without asking the server.
    pub fn fresh(&self, now: u64) -> bool {
        now < self.policy.fresh_until
    }

    /// The headers that ask whether it is still current.
    pub fn validators(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if let Some(etag) = &self.policy.etag {
            headers.push(("If-None-Match".to_string(), etag.clone()));
        }
        if let Some(modified) = &self.policy.last_modified {
            headers.push(("If-Modified-Since".to_string(), modified.clone()));
        }
        headers
    }

    /// The entry as bytes: the magic, the freshness, four strings, then the body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.body.len() + 128);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.policy.fresh_until.to_le_bytes());
        for text in [
            Some(self.url.as_str()),
            self.policy.etag.as_deref(),
            self.policy.last_modified.as_deref(),
        ] {
            let text = text.unwrap_or("");
            out.extend_from_slice(&(text.len() as u32).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        out.extend_from_slice(&self.body);
        out
    }

    /// An entry read back, or `None` for a file that is not one.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let rest = bytes.strip_prefix(MAGIC)?;
        let (fresh, mut rest) = rest.split_at_checked(8)?;
        let fresh_until = u64::from_le_bytes(fresh.try_into().ok()?);
        let mut text = || -> Option<String> {
            let (len, after) = rest.split_at_checked(4)?;
            let len = u32::from_le_bytes(len.try_into().ok()?) as usize;
            let (value, after) = after.split_at_checked(len)?;
            rest = after;
            String::from_utf8(value.to_vec()).ok()
        };
        let url = text()?;
        let some = |s: String| (!s.is_empty()).then_some(s);
        let etag = some(text()?);
        let last_modified = some(text()?);
        Some(Self {
            url,
            policy: Policy {
                store: true,
                fresh_until,
                etag,
                last_modified,
            },
            body: rest.to_vec(),
        })
    }
}

/// The cache's folder, and the budget it is kept under.
pub(crate) struct DiskCache {
    pub dir: PathBuf,
    pub max_bytes: u64,
}

impl DiskCache {
    fn path(&self, url: &str) -> PathBuf {
        self.dir.join(file_name(url))
    }

    /// The entry kept for `url`, if one is and it is that address's. Reading it counts as a
    /// use, for the eviction.
    pub fn read(&self, url: &str) -> Option<Entry> {
        let path = self.path(url);
        let bytes = std::fs::read(&path).ok()?;
        let entry = Entry::decode(&bytes).filter(|e| e.url == url)?;
        touch(&path);
        Some(entry)
    }

    /// Writes `entry`, through a temporary file so that a reader never sees half of one,
    /// then brings the folder back under its budget.
    pub fn write(&self, entry: &Entry) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(&entry.url);
        let temporary = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&temporary, entry.encode())?;
        if let Err(err) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(err);
        }
        self.trim(self.max_bytes);
        Ok(())
    }

    /// Removes the entry for `url`.
    pub fn remove(&self, url: &str) {
        let _ = std::fs::remove_file(self.path(url));
    }

    /// The entries' files: path, size and last use.
    fn files(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|item| {
                let path = item.path();
                if path.extension().and_then(|e| e.to_str()) != Some("img") {
                    return None;
                }
                let meta = item.metadata().ok()?;
                Some((path, meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)))
            })
            .collect()
    }

    /// The bytes the folder holds.
    pub fn bytes(&self) -> u64 {
        self.files().iter().map(|(_, size, _)| size).sum()
    }

    /// Removes the entries used least recently until the folder holds at most `budget`
    /// bytes.
    pub fn trim(&self, budget: u64) {
        let mut files = self.files();
        let mut held: u64 = files.iter().map(|(_, size, _)| size).sum();
        if held <= budget {
            return;
        }
        files.sort_by_key(|(_, _, used)| *used);
        for (path, size, _) in files {
            if held <= budget {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                held -= size;
            }
        }
    }

    /// Removes every entry.
    pub fn clear(&self) {
        self.trim(0);
    }
}

/// Marks `path` as just used.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const NOW: u64 = 1_000_000;

    #[test]
    fn http_dates_in_all_three_forms() {
        let expected = 784_111_777; // 1994-11-06 08:49:37 UTC
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(expected)
        );
        assert_eq!(
            parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"),
            Some(expected)
        );
        assert_eq!(parse_http_date("Sun Nov  6 08:49:37 1994"), Some(expected));
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(
            parse_http_date("Tue, 10 Nov 2026 12:00:00 GMT"),
            Some(1_794_312_000)
        );
        for bad in ["0", "-1", "", "yesterday", "Sun, 32 Nov 1994 08:49:37 GMT"] {
            assert_eq!(parse_http_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn max_age_less_the_age_already_spent() {
        let p = policy(&h(&[("Cache-Control", "public, max-age=600")]), NOW);
        assert!(p.store);
        assert_eq!(p.fresh_until, NOW + 600);
        let p = policy(&h(&[("cache-control", "max-age=600"), ("Age", "100")]), NOW);
        assert_eq!(p.fresh_until, NOW + 500);
        // `max-age` wins over `Expires`.
        let p = policy(
            &h(&[
                ("Cache-Control", "max-age=60"),
                ("Expires", "Thu, 01 Jan 2099 00:00:00 GMT"),
            ]),
            NOW,
        );
        assert_eq!(p.fresh_until, NOW + 60);
    }

    #[test]
    fn no_store_is_never_written_and_no_cache_is_always_asked_about() {
        let p = policy(&h(&[("Cache-Control", "no-store")]), NOW);
        assert!(!p.store);
        let p = policy(
            &h(&[
                ("Cache-Control", "no-cache, max-age=600"),
                ("ETag", "\"a\""),
            ]),
            NOW,
        );
        assert!(p.store);
        assert_eq!(p.fresh_until, NOW, "stale at once: asked about every time");
        assert_eq!(p.etag.as_deref(), Some("\"a\""));
        let p = policy(&h(&[("Cache-Control", "max-age=0")]), NOW);
        assert_eq!(p.fresh_until, NOW);
    }

    #[test]
    fn expires_counts_from_the_servers_date() {
        let date = "Sun, 06 Nov 1994 08:49:37 GMT";
        let expires = "Sun, 06 Nov 1994 09:49:37 GMT";
        let p = policy(&h(&[("Date", date), ("Expires", expires)]), NOW);
        assert_eq!(p.fresh_until, NOW + 3600);
        let p = policy(&h(&[("Expires", "0")]), NOW);
        assert_eq!(p.fresh_until, NOW, "an unreadable Expires is already past");
    }

    #[test]
    fn a_response_that_says_nothing_gets_a_heuristic_or_the_default() {
        // Modified 100 days before its date: fresh for a tenth of that, capped at 7 days.
        let p = policy(
            &h(&[
                ("Date", "Tue, 10 Nov 2026 12:00:00 GMT"),
                ("Last-Modified", "Tue, 10 Nov 2026 02:00:00 GMT"),
            ]),
            NOW,
        );
        assert_eq!(p.fresh_until, NOW + 3600, "a tenth of ten hours");
        let p = policy(
            &h(&[
                ("Date", "Tue, 10 Nov 2026 12:00:00 GMT"),
                ("Last-Modified", "Thu, 01 Jan 2015 00:00:00 GMT"),
            ]),
            NOW,
        );
        assert_eq!(p.fresh_until, NOW + HEURISTIC_CAP.as_secs());
        let p = policy(&[], NOW);
        assert_eq!(p.fresh_until, NOW + DEFAULT_FRESHNESS.as_secs());
    }

    #[test]
    fn validators_ask_with_the_etag_and_the_date() {
        let entry = Entry {
            url: "https://x/a.png".into(),
            policy: policy(
                &h(&[
                    ("ETag", "\"v2\""),
                    ("Last-Modified", "Sun, 06 Nov 1994 08:49:37 GMT"),
                    ("Cache-Control", "max-age=10"),
                ]),
                NOW,
            ),
            body: vec![1, 2, 3],
        };
        assert!(entry.fresh(NOW + 9));
        assert!(!entry.fresh(NOW + 10));
        assert_eq!(
            entry.validators(),
            h(&[
                ("If-None-Match", "\"v2\""),
                ("If-Modified-Since", "Sun, 06 Nov 1994 08:49:37 GMT")
            ])
        );
    }

    #[test]
    fn an_entry_round_trips_and_a_stranger_is_refused() {
        let entry = Entry {
            url: "https://example.com/ada.png?w=200".into(),
            policy: Policy {
                store: true,
                fresh_until: 42,
                etag: Some("\"x\"".into()),
                last_modified: None,
            },
            body: (0..=255).collect(),
        };
        assert_eq!(Entry::decode(&entry.encode()), Some(entry.clone()));
        assert_eq!(Entry::decode(b"not an entry"), None);
        let mut cut = entry.encode();
        cut.truncate(20);
        assert_eq!(Entry::decode(&cut), None, "a torn file is no entry");
        assert_ne!(file_name("https://a/1"), file_name("https://a/2"));
        assert!(file_name("x").ends_with(".img"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("frus-http-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn entry(url: &str, size: usize) -> Entry {
        Entry {
            url: url.into(),
            policy: policy(&[], NOW),
            body: vec![7; size],
        }
    }

    #[test]
    fn the_folder_keeps_entries_and_tells_addresses_apart() {
        let cache = DiskCache {
            dir: scratch("keep"),
            max_bytes: 1 << 20,
        };
        cache.write(&entry("https://a/1", 10)).unwrap();
        assert_eq!(cache.read("https://a/1").unwrap().body, vec![7; 10]);
        assert!(cache.read("https://a/2").is_none());
        // A file of another address under this one's name is not served for it.
        std::fs::write(
            cache.dir.join(file_name("https://a/3")),
            entry("https://a/other", 1).encode(),
        )
        .unwrap();
        assert!(cache.read("https://a/3").is_none());
        cache.remove("https://a/1");
        assert!(cache.read("https://a/1").is_none());
        cache.clear();
        assert_eq!(cache.bytes(), 0);
        let _ = std::fs::remove_dir_all(&cache.dir);
    }

    /// Past its budget the folder loses the entries used least recently, and a read is a
    /// use.
    #[test]
    fn eviction_removes_the_least_recently_used() {
        let dir = scratch("evict");
        let big = DiskCache {
            dir: dir.clone(),
            max_bytes: u64::MAX,
        };
        let base = SystemTime::now() - Duration::from_secs(1000);
        for (i, url) in ["https://a/1", "https://a/2", "https://a/3"]
            .iter()
            .enumerate()
        {
            big.write(&entry(url, 1000)).unwrap();
            // Distinct, ordered times, whatever the file system's resolution.
            let file = std::fs::File::options()
                .write(true)
                .open(dir.join(file_name(url)))
                .unwrap();
            file.set_modified(base + Duration::from_secs(i as u64 * 10))
                .unwrap();
        }
        // The oldest is read, so it becomes the newest.
        assert!(big.read("https://a/1").is_some());
        let one = big.bytes() / 3;
        let small = DiskCache {
            dir: dir.clone(),
            max_bytes: one * 2,
        };
        small.trim(small.max_bytes);
        assert!(small.read("https://a/1").is_some(), "read last: kept");
        assert!(
            small.read("https://a/2").is_none(),
            "least recently used: gone"
        );
        assert!(small.read("https://a/3").is_some());
        assert!(small.bytes() <= one * 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
