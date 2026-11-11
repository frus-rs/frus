# Pictures, a database and signing in

Three things nearly every application that talks to a service needs, and none of which needs a
view of the platform's own: pictures from a server that come back without the network, a
database on the device, and a sign-in through the system's browser.

```toml
[dependencies]
frus = { version = "0.5", features = ["net", "sqlite", "auth"] }
```

| | Feature | Desktop | Android | Web |
| --- | --- | --- | --- | --- |
| [Pictures from a server](#pictures-from-a-server) | `net` | ✓ | ✓ | ✓ (the browser's cache) |
| [A database](#a-database) | `sqlite` | ✓ | ✓ | — |
| [Signing in](#signing-in) | `auth` | ✓ | ✓ | ✓ |

## Pictures from a server

`Image::network(url)` fetches, decodes and shows a picture. What stands in while it comes, and
what is shown if it never does, are the image's to say:

```rust,no_run
use frus::{BoxFit, Icon, Icons, Image, Skeleton};

let avatar = Image::network("https://example.com/avatars/ada.png")
    .size(72.0, 72.0)
    .fit(BoxFit::Cover)
    .placeholder(Skeleton::new())
    .error_placeholder(Icon::new(Icons::BROKEN_IMAGE));
# let _ = avatar;
```

The picture fades in over its placeholder when it arrives. It is decoded at the size it is shown
at, on a background thread, so a list of thumbnails does not hold full-size photographs in
memory.

**Caching.** A picture is kept twice:

- **in memory**, decoded: the pictures used least recently go first once they weigh more than
  100 MiB, and one on screen never goes;
- **on disk**, in the application's cache folder, as the server sent it, for as long as its
  `Cache-Control: max-age`, `Expires` or `Last-Modified` allow (seven days when it says
  nothing). After that it is asked about again with its `ETag` or date, and a `304 Not Modified`
  keeps the copy without downloading it. `no-store` is never written. Without a network, the copy
  on disk is shown even when it is stale. The folder is kept under 100 MiB, the files used least
  recently removed first.

On the web the browser's own cache keeps the files, and the memory cache works as everywhere.

**What it does not do twice.** Two widgets showing the same address, or one address shown at two
sizes, make one download. At most six download at once. A picture that leaves the screen before
its download starts is never downloaded, and one already downloading stops at the next chunk:
a list flung past two hundred pictures downloads the ones it stops on.

`frus::image_cache()` is the handle on all of it:

```rust,no_run
let cache = frus::image_cache();
cache.set_max_disk_bytes(50 * 1024 * 1024);
cache.set_max_memory_bytes(40 * 1024 * 1024);
cache.evict("https://example.com/avatars/ada.png"); // the picture behind it changed
println!("{} bytes on disk, {} decoded", cache.disk_bytes(), cache.memory_bytes());
cache.clear(); // memory and disk
```

## A database

`frus::database(name)` opens SQLite in the application's data folder — the same SQLite on every
platform, compiled in, with nothing to install. The schema is a list of steps; the database
remembers how many it has run and runs the new ones when it opens, each in a transaction of its
own:

```rust,no_run
use frus::{params, Command};

#[derive(Clone)]
enum Msg {
    Opened(Result<frus::Database, frus::DatabaseError>),
}

fn open() -> Command<Msg> {
    Command::perform_async(async {
        Msg::Opened(
            frus::database("notes.db")
                .migrations(&[
                    "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
                    // Shipped steps are never edited: a change is a new step.
                    "ALTER TABLE notes ADD COLUMN done INTEGER NOT NULL DEFAULT 0",
                ])
                .open()
                .await,
        )
    })
}

async fn use_it(db: frus::Database) -> Result<(), frus::DatabaseError> {
    let id = db
        .insert("INSERT INTO notes (body, done) VALUES (?1, ?2)", params!["milk", false])
        .await?;
    for row in db.query("SELECT id, body FROM notes WHERE done = ?1", [false]).await? {
        let body: String = row.get("body")?;
        println!("{id}: {body}");
    }
    // All or nothing: an `Err`, or a panic, rolls every statement back.
    db.transaction(move |tx| {
        tx.execute("UPDATE notes SET done = 1 WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM notes WHERE done = 1 AND id < ?1", [id])?;
        Ok(())
    })
    .await
}
# let _ = (open, use_it);
```

Every call is a future answered by the database's own thread, which starts when the database
opens and ends with the last handle, so a query never holds up a frame. Parameters are typed
(`params![..]` for mixed types, an array or `()` otherwise), and so is reading a column:
`row.get::<T>(name_or_index)` for integers, floats, `bool`, `String`, bytes and `Option` of any
of them. The database is in WAL mode, with its foreign keys enforced.

A file written by a newer version of the application, with more steps than this one knows, is
refused (`DatabaseError::NewerThanApp`) rather than opened with a schema it does not understand.

On the web, `open` answers `DatabaseError::Unsupported`: SQLite there needs a separate
WebAssembly build of its own and a worker with the origin's private file system, a megabyte more
for every page. Keep small things in `frus::prefs()`, and the rest on the server.

The feature adds about a megabyte to an Android application (SQLite itself).

## Signing in

`frus::auth_session` runs the OAuth 2 authorization code flow with PKCE, which is what OpenID
Connect providers ask of an application on a device: the sign-in page opens in the system's
browser, where the person's passwords and passkeys are, and never in a view the application
could read.

```rust,no_run
use frus::{AuthRequest, Command};

#[derive(Clone)]
enum Msg {
    Signed(Result<frus::AuthTokens, frus::AuthError>),
}

fn sign_in() -> Command<Msg> {
    let request = AuthRequest::new(
        "https://accounts.example.com/authorize",
        "my-client-id",
        "http://127.0.0.1/callback",
    )
    .token_url("https://accounts.example.com/token")
    .scope("openid profile offline_access");
    Command::perform_async(async move { Msg::Signed(frus::auth_session(request).await) })
}
# let _ = sign_in;
```

The answer comes back to the redirect address, which decides how:

| Redirect address | How the answer arrives | Where |
| --- | --- | --- |
| `http://127.0.0.1/…`, `http://localhost:8400/…` | A listener on the loopback interface, on that port or, with none, a free one; on Android the page is a Custom Tab and the application comes back over it | The desktops, Android |
| `com.example.app:/oauth` | The link that opens the application | A desktop that registered the scheme; not Android yet (`AuthError::Unsupported`: the browser's link back recreates the activity) |
| `https://your.site/callback.html` | A popup, read once it is back on the page's own origin | The web |

The `state` sent is checked on the way back (an answer that is not this request's is
`AuthError::StateMismatch`), the code is exchanged with the PKCE verifier only this run knows,
and with an `openid` scope a `nonce` is sent and checked in the ID token. A person who refuses
is `AuthError::Denied`, one who closes the popup `AuthError::Cancelled`, and one who walks away
`AuthError::Timeout` after five minutes (`.timeout(..)` changes it).

To stay signed in across launches, keep the refresh token in the system's secret store (with the
`secure-store` feature) and use it at the next start:

```rust,ignore
let request = request.store_refresh_token("refresh");
// … at the next start:
if let Some(refresh) = frus::stored_refresh_token("refresh") {
    let tokens = request.refresh(&refresh).await?;
}
```
