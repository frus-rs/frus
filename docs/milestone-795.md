# Milestone 795 — Network images with a disk cache, a SQLite database, signing in with PKCE

## Objective

Three capabilities a real application needs and none of which needs a view of the platform's
own, each one function re-exported from `frus`:

1. **Network images that survive the network going away.** `Image::network(url)` already
   fetched, decoded at the shown size on background threads, faded in over a placeholder and
   kept a memory cache (milestones 372, 687, 688). What was missing: a **disk cache** that obeys
   HTTP, one download per address however many widgets and sizes ask, a cap on concurrent
   downloads, **cancellation** when the picture leaves the screen, an **error placeholder**, and
   an application-facing handle, `frus::image_cache()`.
2. **A database**: `frus::database("app.db")` — SQLite in the application's data folder, versioned
   migrations, typed parameters and rows, transactions, every call off the interface's thread.
3. **Signing in**: `frus::auth_session(AuthRequest::new(..))` — OAuth 2 / OpenID Connect,
   authorization code with PKCE (RFC 7636, `S256`), through the system's browser as RFC 8252
   asks: loopback listener on the desktops, Custom Tab and the application's link on Android, a
   popup on the web.

## API

```rust
// Images (feature `net`)
Image::network(url)
    .size(72.0, 72.0)
    .placeholder(Skeleton::new())
    .error_placeholder(Icon::new(Icons::BROKEN_IMAGE)); // new: ImagePlaceholder::Icon too
let cache = frus::image_cache();
cache.clear(); cache.clear_memory(); cache.clear_disk(); cache.evict(url);
cache.set_max_disk_bytes(n); cache.set_max_memory_bytes(n);
cache.disk_bytes(); cache.memory_bytes(); cache.disk_dir();
frus::Request::get(url).send_response().await?;  // new: status + headers (frus::Response)

// Database (feature `sqlite`)
let db = frus::database("notes.db").migrations(&[V1, V2]).open().await?;
db.execute(sql, params![..]).await?;   // -> rows changed
db.insert(sql, [..]).await?;           // -> rowid
db.query(sql, ()).await?;              // -> Vec<Row>; row.get::<T>("name" | 0)
db.query_one(sql, ..).await?; db.execute_batch(sql).await?; db.version().await?;
db.transaction(|tx| { tx.execute(..)?; Ok(()) }).await?;

// Sign-in (feature `auth`)
let tokens = frus::auth_session(
    AuthRequest::new(authorize_url, client_id, redirect_uri)
        .token_url(token_url)
        .scope("openid profile offline_access")
        .param("prompt", "login")
        .timeout(Duration::from_secs(300))
        .store_refresh_token("key"),       // with `secure-store`
).await?;
request.refresh(&refresh_token).await?;
frus::stored_refresh_token("key");
```

## Design

### Images

**Where the bytes come from** is now `frus-shell/src/image_cache.rs`, the fetcher the shell
registers (it replaces `fetch_image_bytes` in `app.rs`):

- **Coalescing.** A table of callbacks per address: a second ask for an address already on its
  way — another widget, or the same picture at another decode size, which `frus-core` keys
  separately — joins the first download instead of starting one.
- **Disk cache first** (`http_cache.rs`). One file per address in `<cache>/frus-images`
  (Android: `Context.getCacheDir()`; desktop: `%LOCALAPPDATA%\frus\<program>`,
  `~/Library/Caches/frus/<program>`, `$XDG_CACHE_HOME/frus/<program>`), named by a 64-bit FNV
  hash with the address written inside to rule out collisions, holding the body, the time it is
  fresh until, the `ETag` and the `Last-Modified`. Written through a temporary file and a rename.
  The rules (RFC 9111, as a private cache):
  `no-store` is never written; `no-cache` and `max-age=0` are kept and revalidated every time;
  `max-age` minus `Age`, else `Expires` minus `Date`, else a tenth of `Date − Last-Modified`
  capped at 7 days, else 7 days. A stale entry is revalidated with `If-None-Match` /
  `If-Modified-Since`: `304` renews it without the body; a network error or a 5xx serves the stale
  copy (what makes a second launch with the network off show the pictures); a 4xx removes it.
  Past the budget (100 MiB by default) the least recently used files go — a read touches the
  file's modification time, so "used" means shown, not downloaded.
- **Six downloads at a time** natively (an `async_lock::Semaphore`; already in the tree).
- **Cancellation.** "Nobody asked this frame" alone cannot be the signal, because the tree is
  retained; "the widget was dropped" alone cannot either, because a list builds its rows again
  in each layout and drops the previous copies. Both are used. An `Image` takes an
  `ImageInterest` (an `Arc`, the store keeps a `Weak`) the first time it is **laid out or
  painted** while loading, and `Image::network` marks the address as **seen** in the current
  frame when it is built (`frus_core::image_seen`; the shell counts frames with
  `advance_image_frame`). A fetch stays wanted while a widget that showed it lives, or while it
  was built in the last three frames; a row scrolled out of a list is neither. The fetcher asks
  `frus_core::image_wanted(url)` before taking a download slot and between 64 KiB chunks of the
  body, and answers `IMAGE_FETCH_CANCELLED`; `frus-core` then forgets the entry and bumps the
  landing count so whoever still shows it asks again — or, if a widget took the picture back
  while the answer was on its way, starts the fetch again. An image only asked its `state()`
  and never laid out keeps the old behaviour: never cancelled. Each fetch carries a ticket, so an
  answer for an entry started again since is dropped. The first Windows run, with the interest
  alone, showed why the frame mark is needed: every row but the first stayed on its placeholder,
  their fetches given up as the rows were rebuilt.
- **Rebuild on landing.** A widget laid out while loading keeps its `Loading` answer in its
  `OnceCell`; the shell now counts `frus_core::image_landings()` and builds the tree again when
  it moves (folded into `take_task_arrivals`). Without this a picture that landed showed only
  when something else caused a rebuild.
- **Web**: the browser's fetch and HTTP cache; coalescing, cancellation (checked before the
  request and after the body) and the memory cache are the same code.

`Image::error_placeholder` paints an `ImagePlaceholder` when the picture failed; the placeholder
gained an `Icon` variant for the usual broken-image sign. Both are the widget's to set, so the
look stays the application's.

### Database

`frus-shell/src/database.rs`, behind the `sqlite` feature of `frus` (off by default), on
**rusqlite 0.41 with `bundled`** (MIT, the de facto SQLite binding, monthly releases): SQLite is
compiled from its amalgamation by `cc`, so Android, Windows, macOS and Linux run the same
version with no system library. One connection, owned by a thread named `frus-sqlite` that is
started by `open` and ends when the last `Database` clone drops its sender — nothing exists until
an application opens a database. Each call sends a closure to that thread and awaits a one-shot
`async_channel` answer, so it works from the shell's executor and from any other executor; a
panic in the closure is caught and reported, and the thread keeps serving.

Migrations are numbered by position and recorded in `PRAGMA user_version`; each runs in its own
transaction with the version bump, so a failing step leaves the database at the previous version
(`DatabaseError::Migration { version, .. }`), and a file at a higher version than the application
knows is refused (`NewerThanApp`). The connection is opened in WAL mode, `synchronous=NORMAL`,
`foreign_keys=ON`, `busy_timeout=5000`. Statements go through `prepare_cached`.

Values are SQLite's five types (`Value`), parameters a `Params` made from `params![..]`, an array,
a `Vec<Value>` or `()`; `Row::get::<T>` takes a position or a name and converts to the integer
types (range-checked), `bool`, floats, `String`, `Vec<u8>` and `Option<T>`.

**The web: `Unsupported`, deliberately.** rusqlite's bundled C does not build for
`wasm32-unknown-unknown` (no libc); the alternative is the official sqlite-wasm build, which
needs its own ~1 MB WebAssembly module, a dedicated worker, and the Origin Private File System's
synchronous access handles for durability — a second runtime beside the application's, for every
page. An IndexedDB key-value store behind the same API would accept SQL it cannot run. So `open`
answers `DatabaseError::Unsupported` on the web, and the docs point to `prefs()` and the server.
The dependency is target-gated, so a web build with the feature carries none of it.

### Sign-in

`frus-shell/src/auth.rs`, behind `auth` (implies `net` + `json`). Hashing and randomness come
from **ring** natively — already in every `net` build through rustls, so it adds nothing — and
from `crypto.subtle` / `crypto.getRandomValues` on the web. Base64url and form encoding are a
few lines each and tested against RFC examples.

- `state` (16 random bytes), PKCE verifier (32 bytes → 43 characters), `S256` challenge; with an
  `openid` scope a `nonce`, checked in the ID token's payload. The ID token's signature is not
  verified: it comes straight from the token endpoint over TLS, which OpenID Connect Core
  §3.1.3.7 accepts in its place.
- The answer is checked for `state` **before** anything else, so a forged `error=` is reported
  as `StateMismatch`, not as the server's refusal.
- **Loopback** (`http://127.0.0.1/…`, `localhost`, `[::1]`): a `TcpListener` on that port, or on
  a free one with the redirect address rewritten (RFC 8252 §7.3); it answers favicon requests
  with 404 and the redirect with a "you can close this window" page (`.success_page(html)`).
- **Links** (any other scheme): a single waiting sign-in in a process-wide slot; the shell's link
  intake (`open_links_from_other_instances`) offers each incoming link to it before routing it —
  a desktop application that registered its scheme. **On Android this path answers
  `AuthError::Unsupported`**, after two device runs: Chrome hands the redirect link back with
  `FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_CLEAR_TOP` (`flg=0x14000000` in the activity manager's
  log), from a Custom Tab and from an ordinary tab alike, and with the native activity's
  standard launch mode that **finishes and recreates** the running activity — the application
  started over, its sign-in lost. Taking the link as `onNewIntent` needs an activity class of
  frus's own (see Not done).
- **Loopback on Android**: allowed too, and that path does use a **Custom Tab** — the intent
  carries the Custom Tabs session extra (`android.support.customtabs.extra.SESSION`, a null
  binder), all Chrome needs, with no androidx library and no dex change. The application's own
  listener receives the answer, then starts its activity with `CLEAR_TOP | SINGLE_TOP`, which
  removes the tab above it and hands the intent to the running activity instead of recreating it
  (Android 10 allows the start: the application's activity is in the foreground task, under the
  tab).
- **Web**: a popup (`window.open`, inside the click's transient activation), polled every 200 ms;
  its address is unreadable while on the provider's origin, and read and closed once it is back
  on the page's own (the redirect address must be of this origin).
- Tokens: `access_token`, `token_type`, `expires_in`, `refresh_token`, `id_token`, `scope`;
  `refresh()` for the refresh grant; `store_refresh_token(key)` keeps it in `secure_store()`.

## How it was tested

### Unit tests

- `http_cache` (frus-shell): the three HTTP date forms, `max-age` less `Age`, `max-age` over
  `Expires`, `no-store` / `no-cache` / `max-age=0`, `Expires` from the server's `Date`, the
  `Last-Modified` heuristic and its cap, the default, validators, an entry's round trip and a
  torn or foreign file refused, and least-recently-used eviction where a read counts as a use.
- `image_cache` (frus-shell), over a real socket: a fresh entry served from disk (one request
  for two loads), a `no-cache` entry revalidated with `If-None-Match` and served on `304`,
  `no-store` never written, a stale copy served with the server gone, and three asks for one
  address in flight answered by **one** request.
- `image` (frus-widgets): a fetch stays wanted while its widget lives or was built in the last
  frames and stops being wanted after; an image only asked its state is never cancelled; a
  cancelled fetch is forgotten and fetched again when shown; a cancellation that crosses a new
  widget starts the fetch again and the picture lands; the error placeholder is painted for a
  failed picture and nothing without it.
- `database` (frus-shell): migrations run once each and in order across opens, a newer file is
  refused, a failed step leaves the version before it, typed parameters and rows (integers
  narrowed with range checks, reals, text, blobs, `NULL`/`Option`, wrong type and missing
  column errors), a transaction kept whole or undone by an `Err` and by a panic (the thread
  keeps serving), and a hundred calls from many tasks.
- `auth` (frus-shell): RFC 7636 appendix B's verifier and challenge, base64url at every length,
  43-character random secrets, the authorization address and its encoding, `state` checked before
  `error`, redirect kinds, link matching, the waiting slot, token answers and errors, the ID
  token's `nonce`, percent coding.

### Checks

`cargo clippy -D warnings` for the host (frus-core, frus-widgets, frus-shell with `net`, `json`,
`sqlite`, `auth`, `secure-store`, frus, the example, all targets), for `aarch64-linux-android`
(frus-shell, the NDK's clang) and for `wasm32-unknown-unknown` (frus-shell and the example);
frus-shell's library tests (595), frus-widgets' (2323) and its `core_exports` and
`thread_locals` guards, frus-core's, frus-shell's and frus's doctests (the new guide page
included), and `cargo doc` with `-D warnings`.

### Windows (real window, mock server)

A Python mock server in the scratch folder serves 200 PNGs of 256x256 (186 KB each,
`Cache-Control: max-age=3600`, `ETag`, every 50th a 404) and an authorization server that checks
every parameter and the PKCE verifier against the challenge, and issues an ID token with the
`nonce`.

- Images: the list shows its pictures (before the frame mark, only the first did — see
  Cancellation). With the server **stopped**, a second launch shows the same pictures from the
  disk cache (`%LOCALAPPDATA%rusrus-data-examplerus-images`, 7 files, 3.1 MB).
- Database: `notes.db` in `%APPDATA%rus-data-example`; a hundred inserts in one transaction
  in 0.6 ms; Python's `sqlite3` reads the file afterwards: 100 rows, `user_version` 2, WAL.
- Sign-in: the default browser opened on the loopback redirect (a free port, 51062), the server
  saw `state`, `S256` challenge and `nonce`, the token request passed the PKCE check, the ID
  token's `nonce` matched, and the refresh token is in the Credential Manager
  (`frus.frus-data-example/frus-data-example-refresh`).

### Phone (Huawei STK-L21, Android 10, release APK)

- **200 network images, flung end to end** with 25 swipes: 120 frames in 2.0 s (59 fps) while
  scrolling and loading, interval median 16.7-16.9 ms, 95th 18.6-20.7 ms, 0-3 frames over
  budget per 2 s window; layout and paint 2.3-2.4 ms median, render 13 ms. The server saw **46
  requests for 200 rows**; about 15 completed (memory 2160 KiB decoded, disk 2734 KiB) — the
  rest were given up as their rows left the screen, before or during their download. Pictures
  were decoded at 192x192 for a 72-dp box at density 2.625, not at 256x256. The 404 row showed
  the broken-picture sign. Memory: PSS 269 MB, of which 199 MB graphics, Native heap 16.5 MB.
- **Second launch with the server unreachable** (`adb reverse` removed): the first eight pictures
  shown at once from `/data/user/0/com.frus.data/cache/frus-images`.
- **Database**: `files/notes.db`; a hundred inserts in one transaction in 3.5 ms; after a
  force-stop and a new start, the 100 notes are there.
- **Sign-in, loopback in a Custom Tab**: the tab opened in the application's task, the
  application's listener received the redirect, the activity came back over the tab without
  being recreated (the same `ActivityRecord` resumed), tokens exchanged with PKCE, `nonce`
  checked, refresh token kept in the Android Keystore; **Refresh** then got new tokens from it.
- **Sign-in, custom scheme**: failed twice as described under Design, then made `Unsupported`.

### Size

The example's release APK for arm64, built twice with the same NDK (27) and profile: once as it
is, once with the `sqlite` feature off and its database calls replaced by stubs that fail.

| | APK | `libfrus_data_example.so` |
| --- | --- | --- |
| with `sqlite` | 8,982,957 B | 18,641,592 B |
| without | 7,983,533 B | 16,653,832 B |
| **`sqlite` costs** | **+999,424 B (0.95 MiB)** | +1,987,760 B |

That is SQLite itself, compiled in. The `auth` feature adds no dependency to a `net` build (its
hashing and random bytes are `ring`'s, already there for TLS). With none of the three features
an application links none of this: `frus-hello`, which `ci/size-budget.toml` measures, turns on
none of them; the code it does get is a frame counter and a store lookup in `Image::network`. The
budget was not re-measured here (WSL, where the size job's recipe runs, was down: C: full).

### Not run

- The web: the code compiles for `wasm32-unknown-unknown` (clippy, `frus-shell` with `net`,
  `sqlite`, `auth`, and the example), but the popup sign-in and the images were **not run in a
  browser** in this milestone.
- macOS, Linux, iOS: not built here (iOS paths answer `Unsupported` for links; the loopback and
  database code is shared with the desktops).


## Not done

- **A custom-scheme redirect on Android.** It needs `onNewIntent`, which the native activity
  does not forward; an activity class of frus's own (or a launch mode plus a bridge to read the
  new intent) would let Chrome's `CLEAR_TOP` hand the link to the running activity. Until then
  Android sign-ins use a loopback redirect, which some providers refuse for Android clients.
- **The web, in a browser**: not run (see Not run). The popup flow also needs the redirect page
  on the application's own origin and a token endpoint that allows CORS.
- **SQLite on the web**: `Unsupported` by design (see Design).
- **OpenID discovery** (`.well-known/openid-configuration`), ID token **signature** checks
  (the token comes straight from the token endpoint over TLS), confidential clients, token
  revocation and automatic refresh before expiry.
- **Cache-Control for the image cache in a 304 without directives** falls back to the default
  rules rather than the stored response's `max-age`, which is not kept.
- **Idle CPU after the image list** was not measured: the example's statistics build logs the
  cache every three seconds, which is a timer of its own.
- The image disk cache's size is recomputed by listing the folder after each write; fine for a
  few thousand files, not for a hundred thousand.
- A sign-in is not kept across the process being killed while the browser is in front.

