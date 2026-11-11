// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Where `Image::network` gets its bytes** (milestone 795): the disk cache, then the
//! network, one download per address however many widgets and sizes ask, at most
//! [`DOWNLOADS`] at a time, and given up when nobody shows the picture any more.
//!
//! [`image_cache`] is the application's handle on it: clearing it, sizing it, asking how
//! big it is.

use std::collections::HashMap;
#[cfg(not(web))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[cfg(not(web))]
use crate::http_cache::{self, DiskCache};

/// What frus-core hands the fetcher, to be called once with the bytes or why not.
type Deliver = Box<dyn FnOnce(Result<Vec<u8>, String>) + Send + 'static>;

/// Long enough for a photograph on a slow connection, short enough that a dead server does
/// not pin a screen redrawing until the user closes the application.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// How many images download at once, natively: what a browser allows one server. A list
/// flung past two hundred pictures asks for most of them; the ones still waiting here when
/// they leave the screen are never downloaded at all. The Web leaves this to the browser.
#[cfg(not(web))]
pub(crate) const DOWNLOADS: usize = 6;

/// The callbacks waiting for each address being fetched: the second widget, or the second
/// size, of a picture already on its way joins the first's download.
static WAITING: Mutex<Option<HashMap<String, Vec<Deliver>>>> = Mutex::new(None);

/// The budget of the disk cache, in bytes.
#[cfg(not(web))]
static MAX_DISK: AtomicU64 = AtomicU64::new(http_cache::DEFAULT_DISK_CACHE_BYTES);

/// The folder the images are kept in, natively: `frus-images` in the application's cache
/// folder.
#[cfg(not(web))]
fn disk() -> Option<DiskCache> {
    #[cfg(test)]
    if let Some(dir) = TEST_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(DiskCache {
            dir,
            max_bytes: MAX_DISK.load(Ordering::Relaxed),
        });
    }
    Some(DiskCache {
        dir: crate::cache_dir::app_cache_dir()?.join("frus-images"),
        max_bytes: MAX_DISK.load(Ordering::Relaxed),
    })
}

#[cfg(all(test, not(web)))]
static TEST_DIR: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// The fetcher the shell registers for `Image::network`.
pub(crate) fn fetch_image(url: &str, deliver: Deliver) {
    {
        let mut guard = WAITING.lock().unwrap_or_else(|e| e.into_inner());
        let waiting = guard.get_or_insert_with(HashMap::new);
        if let Some(others) = waiting.get_mut(url) {
            others.push(deliver);
            return;
        }
        waiting.insert(url.to_string(), vec![deliver]);
    }
    let url = url.to_string();
    let run = async move {
        let result = load(&url).await;
        let delivers = WAITING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .and_then(|waiting| waiting.remove(&url))
            .unwrap_or_default();
        for deliver in delivers {
            deliver(result.clone());
        }
    };
    #[cfg(not(web))]
    crate::runtime::spawn(run).detach();
    #[cfg(web)]
    wasm_bindgen_futures::spawn_local(run);
}

/// The bytes at `url`: from the disk when the copy there is fresh, from the server
/// otherwise, asking it about the copy when there is one.
#[cfg(not(web))]
async fn load(url: &str) -> Result<Vec<u8>, String> {
    let kept = match disk() {
        Some(folder) => {
            let address = url.to_string();
            blocking::unblock(move || folder.read(&address)).await
        }
        None => None,
    };
    let now = http_cache::unix(std::time::SystemTime::now());
    if let Some(entry) = &kept {
        if entry.fresh(now) {
            return Ok(entry.body.clone());
        }
    }
    let answer = download(url, kept.as_ref().map(|e| e.validators())).await;
    let now = http_cache::unix(std::time::SystemTime::now());
    let write = |entry: http_cache::Entry| {
        if let Some(folder) = disk() {
            blocking::unblock(move || {
                if let Err(err) = folder.write(&entry) {
                    log::warn!("frus: an image could not be kept on disk: {err}");
                }
            })
            .detach();
        }
    };
    match answer {
        // Still current: the copy is shown, fresh again for as long as the server now says.
        Ok(response) if response.status() == 304 => {
            let Some(mut entry) = kept else {
                return Err("HTTP status 304 for an image with no copy kept".to_string());
            };
            let renewed = http_cache::policy(response.headers(), now);
            entry.policy.fresh_until = renewed.fresh_until;
            entry.policy.etag = renewed.etag.or(entry.policy.etag);
            entry.policy.last_modified = renewed.last_modified.or(entry.policy.last_modified);
            let body = entry.body.clone();
            if renewed.store {
                write(entry);
            }
            Ok(body)
        }
        Ok(response) => {
            let policy = http_cache::policy(response.headers(), now);
            let body = response.into_body();
            if policy.store {
                write(http_cache::Entry {
                    url: url.to_string(),
                    policy,
                    body: body.clone(),
                });
            } else if kept.is_some() {
                if let Some(folder) = disk() {
                    folder.remove(url);
                }
            }
            Ok(body)
        }
        Err(crate::FetchError::Cancelled) => Err(frus_widgets::IMAGE_FETCH_CANCELLED.to_string()),
        // Gone, or never there: the copy goes too.
        Err(crate::FetchError::Status(code)) if (400..500).contains(&code) => {
            if kept.is_some() {
                if let Some(folder) = disk() {
                    folder.remove(url);
                }
            }
            Err(crate::FetchError::Status(code).to_string())
        }
        // The network is not there, or the server is failing: a stale copy is better than
        // nothing.
        Err(err) => match kept {
            Some(entry) => Ok(entry.body),
            None => Err(err.to_string()),
        },
    }
}

/// One request to the server, once a download slot is free, unless nobody wants the
/// picture by then.
#[cfg(not(web))]
async fn download(
    url: &str,
    validators: Option<Vec<(String, String)>>,
) -> Result<crate::net::Response, crate::FetchError> {
    static SLOTS: async_lock::Semaphore = async_lock::Semaphore::new(DOWNLOADS);
    let _slot = SLOTS.acquire().await;
    let mut request = crate::Request::get(url).timeout(DEADLINE);
    for (name, value) in validators.unwrap_or_default() {
        request = request.header(name, value);
    }
    let address = url.to_string();
    request
        .send_response_while(move || frus_widgets::image_wanted(&address))
        .await
}

/// The Web: the browser's own cache stands where the disk cache would, and answers the
/// conditional requests itself.
#[cfg(web)]
async fn load(url: &str) -> Result<Vec<u8>, String> {
    let address = url.to_string();
    match crate::Request::get(url)
        .timeout(DEADLINE)
        .send_response_while(move || frus_widgets::image_wanted(&address))
        .await
    {
        Ok(response) => Ok(response.into_body()),
        Err(crate::FetchError::Cancelled) => Err(frus_widgets::IMAGE_FETCH_CANCELLED.to_string()),
        Err(err) => Err(err.to_string()),
    }
}

/// **The cache of network images** (milestone 795): what [`Image::network`] keeps in
/// memory, decoded, and on disk, as the server sent it.
///
/// - In memory, the decoded pictures used least recently go first past
///   [`set_max_memory_bytes`](ImageCache::set_max_memory_bytes) (100 MiB by default); one
///   on screen never goes.
/// - On disk, in the application's cache folder, each response is kept as long as its
///   `Cache-Control`, `Expires` or `Last-Modified` allow, then asked about again with its
///   `ETag`; past [`set_max_disk_bytes`](ImageCache::set_max_disk_bytes)
///   ([`DEFAULT_DISK_CACHE_BYTES`](crate::DEFAULT_DISK_CACHE_BYTES) by default) the files
///   used least recently are removed. A picture shown once is shown again with the network
///   off.
/// - On the Web the browser's own cache stands for the disk.
///
/// ```no_run
/// let cache = frus_shell::image_cache();
/// cache.set_max_disk_bytes(50 * 1024 * 1024);
/// println!("{} bytes on disk", cache.disk_bytes());
/// cache.clear();
/// ```
///
/// [`Image::network`]: frus_widgets::Image::network
#[derive(Clone, Copy, Debug)]
pub struct ImageCache {
    _private: (),
}

/// The application's [`ImageCache`].
///
/// ```no_run
/// frus_shell::image_cache().evict("https://example.com/avatar/ada.png");
/// ```
pub fn image_cache() -> ImageCache {
    ImageCache { _private: () }
}

impl ImageCache {
    /// Forgets every picture: the decoded ones nobody is showing, the remembered failures,
    /// and every file on disk. What is on screen stays until it leaves.
    ///
    /// ```no_run
    /// frus_shell::image_cache().clear();
    /// ```
    pub fn clear(&self) {
        self.clear_memory();
        self.clear_disk();
    }

    /// Forgets the decoded pictures nobody is showing and the remembered failures; the
    /// files on disk stay, so they come back without the network.
    ///
    /// ```no_run
    /// frus_shell::image_cache().clear_memory();
    /// ```
    pub fn clear_memory(&self) {
        frus_widgets::clear_fetched_images();
    }

    /// Removes every file of the disk cache. Nothing on the Web, where the browser keeps
    /// its own.
    ///
    /// ```no_run
    /// frus_shell::image_cache().clear_disk();
    /// ```
    pub fn clear_disk(&self) {
        #[cfg(not(web))]
        if let Some(folder) = disk() {
            folder.clear();
        }
    }

    /// Forgets one address, in memory and on disk, so that it is fetched again the next
    /// time it is shown: the picture behind it changed.
    ///
    /// ```no_run
    /// frus_shell::image_cache().evict("https://example.com/avatar/ada.png");
    /// ```
    pub fn evict(&self, url: &str) {
        frus_widgets::forget_fetched_image(url);
        #[cfg(not(web))]
        if let Some(folder) = disk() {
            folder.remove(url);
        }
    }

    /// The bytes the disk cache's files hold; `0` on the Web.
    ///
    /// ```no_run
    /// let bytes = frus_shell::image_cache().disk_bytes();
    /// # let _ = bytes;
    /// ```
    pub fn disk_bytes(&self) -> u64 {
        #[cfg(not(web))]
        {
            disk().map_or(0, |folder| folder.bytes())
        }
        #[cfg(web)]
        {
            0
        }
    }

    /// The decoded pictures held in memory, in bytes.
    ///
    /// ```no_run
    /// let bytes = frus_shell::image_cache().memory_bytes();
    /// # let _ = bytes;
    /// ```
    pub fn memory_bytes(&self) -> usize {
        frus_widgets::image_cache_bytes()
    }

    /// Keeps the disk cache under `bytes`, removing the files used least recently now if
    /// it is over. `0` keeps nothing on disk.
    ///
    /// ```no_run
    /// frus_shell::image_cache().set_max_disk_bytes(20 * 1024 * 1024);
    /// ```
    pub fn set_max_disk_bytes(&self, bytes: u64) {
        #[cfg(not(web))]
        {
            MAX_DISK.store(bytes, Ordering::Relaxed);
            if let Some(folder) = disk() {
                blocking::unblock(move || folder.trim(bytes)).detach();
            }
        }
        #[cfg(web)]
        let _ = bytes;
    }

    /// Keeps the decoded pictures under `bytes` of memory; one on screen is never dropped.
    ///
    /// ```no_run
    /// frus_shell::image_cache().set_max_memory_bytes(40 * 1024 * 1024);
    /// ```
    pub fn set_max_memory_bytes(&self, bytes: usize) {
        frus_widgets::set_image_cache_budget(bytes);
    }

    /// The folder the disk cache keeps its files in; `None` on the Web, or where the
    /// platform names no cache folder.
    ///
    /// ```no_run
    /// println!("{:?}", frus_shell::image_cache().disk_dir());
    /// ```
    pub fn disk_dir(&self) -> Option<std::path::PathBuf> {
        #[cfg(not(web))]
        {
            disk().map(|folder| folder.dir)
        }
        #[cfg(web)]
        {
            None
        }
    }
}

#[cfg(all(test, not(web)))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// A server answering every request with `respond`, counting them.
    fn server(respond: impl Fn(&str) -> String + Send + 'static) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                counted.fetch_add(1, Ordering::SeqCst);
                let _ = stream.write_all(respond(&request).as_bytes());
            }
        });
        (address, count)
    }

    fn ok(headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    /// One test, since the folder and the waiting table are the process's.
    #[test]
    fn disk_cache_revalidation_and_coalescing_over_a_real_socket() {
        let dir = std::env::temp_dir().join(format!("frus-image-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        *TEST_DIR.lock().unwrap() = Some(dir.clone());
        let run = |url: String| futures_lite::future::block_on(load(&url));

        // Fresh for an hour: the second load is the disk's.
        let (fresh, hits) = server(|_| ok("Cache-Control: max-age=3600\r\n", "fresh"));
        assert_eq!(run(format!("{fresh}/a")).unwrap(), b"fresh");
        // The write is on the blocking pool: wait for the file.
        let file = dir.join(http_cache::file_name(&format!("{fresh}/a")));
        for _ in 0..200 {
            if file.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(run(format!("{fresh}/a")).unwrap(), b"fresh");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the second came from disk");

        // `no-cache` with an ETag: asked about every time, answered 304, served from disk.
        let (revalidated, asked) = server(|request| {
            if request.contains("If-None-Match: \"v1\"") {
                "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n".to_string()
            } else {
                ok("ETag: \"v1\"\r\nCache-Control: no-cache\r\n", "body-v1")
            }
        });
        let url = format!("{revalidated}/b");
        assert_eq!(run(url.clone()).unwrap(), b"body-v1");
        let file = dir.join(http_cache::file_name(&url));
        for _ in 0..200 {
            if file.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            run(url.clone()).unwrap(),
            b"body-v1",
            "the 304 serves the copy"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);

        // `no-store` is never written.
        let (unstored, _) = server(|_| ok("Cache-Control: no-store\r\n", "secret"));
        let url = format!("{unstored}/c");
        assert_eq!(run(url.clone()).unwrap(), b"secret");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!dir.join(http_cache::file_name(&url)).exists());

        // The server gone: the stale copy of the revalidated one is still shown.
        let gone = format!("{revalidated}/b").replace(
            revalidated.rsplit(':').next().unwrap(),
            "1", // port 1: nothing listens there
        );
        let stale = http_cache::Entry {
            url: gone.clone(),
            policy: http_cache::policy(&[("Cache-Control".into(), "max-age=0".into())], 0),
            body: b"stale".to_vec(),
        };
        disk().unwrap().write(&stale).unwrap();
        assert_eq!(run(gone).unwrap(), b"stale", "stale beats nothing offline");

        // Coalescing: three asks for one address while it is in flight are one request.
        let (slow, served) = server(|_| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            ok("Cache-Control: no-store\r\n", "slow")
        });
        let url = format!("{slow}/d");
        let (sender, receiver) = std::sync::mpsc::channel();
        for _ in 0..3 {
            let sender = sender.clone();
            fetch_image(&url, Box::new(move |result| sender.send(result).unwrap()));
        }
        for _ in 0..3 {
            let got = receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            assert_eq!(got.unwrap(), b"slow");
        }
        assert_eq!(served.load(Ordering::SeqCst), 1, "one download for three");

        *TEST_DIR.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
