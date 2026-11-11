// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-data-example` — three capabilities that need no native view, one screen each:
//!
//! - **Images**: two hundred `Image::network` pictures in a list, each with a placeholder and
//!   an error sign, served from the disk cache once seen (`frus::image_cache()`);
//! - **Notes**: a SQLite database (`frus::database`) with two migrations, typed rows and a
//!   transaction;
//! - **Sign in**: `frus::auth_session`, the authorization code with PKCE, through the system's
//!   browser (a Custom Tab on Android, a popup on the Web).
//!
//! The server it talks to is named at build time: `frus run --define DATA_SERVER=http://…`
//! (default `http://127.0.0.1:8795`, the mock server of milestone 795's checks).

#![deny(missing_docs)]

use frus::{
    button, column, row, text, Align, Application, BoxFit, Command, Container, Database,
    DatabaseError, Icon, Icons, Image, ListView, MediaQuery, Size, Skeleton, TextField, Theme,
    Variant, Widget,
};

/// The server the pictures and the sign-in come from.
const SERVER: &str = frus::define!("DATA_SERVER", "http://127.0.0.1:8795");

/// How many pictures the list holds.
const PICTURES: usize = 200;

/// The schema, one step per version.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
    "ALTER TABLE notes ADD COLUMN done INTEGER NOT NULL DEFAULT 0",
];

/// The address the sign-in comes back to on this platform.
fn redirect_uri() -> String {
    if cfg!(target_arch = "wasm32") {
        format!("{SERVER}/callback.html")
    } else {
        "http://127.0.0.1/callback".to_string()
    }
}

/// The sign-in asked for, answered at `redirect`.
fn sign_in_request(redirect: String) -> frus::AuthRequest {
    frus::AuthRequest::new(format!("{SERVER}/authorize"), "frus-data-example", redirect)
        .token_url(format!("{SERVER}/token"))
        .scope("openid profile offline_access")
        .store_refresh_token("frus-data-example-refresh")
}

/// Which screen is shown.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Images,
    Notes,
    SignIn,
}

/// A note as the list shows it.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    id: i64,
    body: String,
    done: bool,
}

#[derive(Default)]
struct DataExample {
    page: Page,
    /// The cache's sizes, as last asked.
    cache: String,
    db: Option<Database>,
    db_status: String,
    notes: Vec<Note>,
    draft: String,
    auth: String,
}

/// What the interface and the effects send.
#[derive(Clone)]
enum Msg {
    Show(Page),
    Measure,
    ClearCache,
    Opened(Result<Database, DatabaseError>),
    Draft(String),
    Add,
    AddHundred,
    Toggle(i64),
    Loaded(Result<Vec<Note>, DatabaseError>),
    SignIn,
    /// The same, on the loopback interface whatever the platform.
    SignInLoopback,
    Refresh,
    Signed(Result<frus::AuthTokens, frus::AuthError>),
}

/// The notes, newest first.
async fn notes(db: Database) -> Result<Vec<Note>, DatabaseError> {
    db.query(
        "SELECT id, body, done FROM notes ORDER BY id DESC LIMIT 100",
        (),
    )
    .await?
    .iter()
    .map(|row| {
        Ok(Note {
            id: row.get("id")?,
            body: row.get("body")?,
            done: row.get("done")?,
        })
    })
    .collect()
}

impl DataExample {
    fn reload(&self) -> Command<Msg> {
        match self.db.clone() {
            Some(db) => Command::perform_async(async move { Msg::Loaded(notes(db).await) }),
            None => Command::none(),
        }
    }
}

impl Application for DataExample {
    type Message = Msg;

    fn init(&mut self) -> Command<Msg> {
        frus::log::info!("data example: server {SERVER}");
        let open = Command::perform_async(async {
            Msg::Opened(
                frus::database("notes.db")
                    .migrations(MIGRATIONS)
                    .open()
                    .await,
            )
        });
        // What a check run does by itself: `DATA_AUTO=signin` (or `notes`) in the environment.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        match std::env::var("DATA_AUTO").as_deref() {
            Ok("signin") => {
                self.page = Page::SignIn;
                return Command::batch([open, self.update(Msg::SignIn)]);
            }
            Ok("notes") => self.page = Page::Notes,
            _ => {}
        }
        open
    }

    /// The cache's sizes in the log every three seconds, in a build given
    /// `--define DATA_STATS=1`: what the checks read.
    fn subscription(&self) -> frus::Subscription<Msg> {
        if frus::define!("DATA_STATS", "0") == "1" {
            frus::Subscription::every(std::time::Duration::from_secs(3), |_| Msg::Measure)
        } else {
            frus::Subscription::none()
        }
    }

    fn update(&mut self, message: Msg) -> Command<Msg> {
        match message {
            Msg::Show(page) => self.page = page,
            Msg::Measure => {
                let cache = frus::image_cache();
                self.cache = format!(
                    "memory {} KiB, disk {} KiB",
                    cache.memory_bytes() / 1024,
                    cache.disk_bytes() / 1024
                );
                frus::log::info!("image cache: {} in {:?}", self.cache, cache.disk_dir());
            }
            Msg::ClearCache => {
                frus::image_cache().clear();
                return self.update(Msg::Measure);
            }
            Msg::Opened(Ok(db)) => {
                self.db_status = format!("{:?}", db.path());
                frus::log::info!("database open at {}", self.db_status);
                self.db = Some(db);
                #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
                if std::env::var("DATA_AUTO").as_deref() == Ok("notes") {
                    return self.update(Msg::AddHundred);
                }
                return self.reload();
            }
            Msg::Opened(Err(err)) => {
                frus::log::warn!("database: {err}");
                self.db_status = err.to_string();
            }
            Msg::Draft(text) => self.draft = text,
            Msg::Add => {
                let (Some(db), body) = (self.db.clone(), std::mem::take(&mut self.draft)) else {
                    return Command::none();
                };
                let body = if body.trim().is_empty() {
                    "A note".to_string()
                } else {
                    body
                };
                return Command::perform_async(async move {
                    match db
                        .insert("INSERT INTO notes (body) VALUES (?1)", [body])
                        .await
                    {
                        Ok(_) => Msg::Loaded(notes(db).await),
                        Err(err) => Msg::Loaded(Err(err)),
                    }
                });
            }
            Msg::AddHundred => {
                let Some(db) = self.db.clone() else {
                    return Command::none();
                };
                return Command::perform_async(async move {
                    let started = std::time::Instant::now();
                    let written = db
                        .transaction(|tx| {
                            for i in 0..100 {
                                tx.execute(
                                    "INSERT INTO notes (body) VALUES (?1)",
                                    [format!("Batch note {i}")],
                                )?;
                            }
                            Ok(())
                        })
                        .await;
                    frus::log::info!(
                        "100 inserts in one transaction: {:?} in {:.1} ms",
                        written,
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                    Msg::Loaded(notes(db).await)
                });
            }
            Msg::Toggle(id) => {
                let Some(db) = self.db.clone() else {
                    return Command::none();
                };
                return Command::perform_async(async move {
                    let _ = db
                        .execute("UPDATE notes SET done = 1 - done WHERE id = ?1", [id])
                        .await;
                    Msg::Loaded(notes(db).await)
                });
            }
            Msg::Loaded(Ok(notes)) => {
                frus::log::info!("notes: {} shown", notes.len());
                self.notes = notes;
            }
            Msg::Loaded(Err(err)) => self.db_status = err.to_string(),
            Msg::SignIn => {
                self.auth = "Waiting for the browser…".to_string();
                return Command::perform_async(async {
                    Msg::Signed(frus::auth_session(sign_in_request(redirect_uri())).await)
                });
            }
            Msg::SignInLoopback => {
                self.auth = "Waiting for the browser…".to_string();
                return Command::perform_async(async {
                    let redirect = "http://127.0.0.1/callback".to_string();
                    Msg::Signed(frus::auth_session(sign_in_request(redirect)).await)
                });
            }
            Msg::Refresh => {
                let Some(refresh) = frus::stored_refresh_token("frus-data-example-refresh") else {
                    self.auth = "No refresh token kept yet".to_string();
                    return Command::none();
                };
                return Command::perform_async(async move {
                    Msg::Signed(sign_in_request(redirect_uri()).refresh(&refresh).await)
                });
            }
            Msg::Signed(result) => {
                self.auth = match result {
                    Ok(tokens) => format!(
                        "Signed in: access token {}…, refresh token {}, ID token {}",
                        tokens.access_token.chars().take(12).collect::<String>(),
                        if tokens.refresh_token.is_some() {
                            "kept"
                        } else {
                            "none"
                        },
                        if tokens.id_token.is_some() {
                            "checked"
                        } else {
                            "none"
                        },
                    ),
                    Err(err) => format!("Not signed in: {err}"),
                };
                frus::log::info!("auth: {}", self.auth);
            }
        }
        Command::none()
    }

    fn view(&self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let Size { width, height } = MediaQuery::of().size;
        let tab = |label: &'static str, page: Page| {
            button(label, Msg::Show(page)).variant(if self.page == page {
                Variant::Filled
            } else {
                Variant::Outlined
            })
        };
        let tabs = row![
            tab("Images", Page::Images),
            tab("Notes", Page::Notes),
            tab("Sign in", Page::SignIn),
        ]
        .gap(8.0);
        let body: Box<dyn Widget<Msg>> = match self.page {
            Page::Images => {
                let side = 72.0;
                let list = ListView::new(PICTURES, side + 12.0, move |i| {
                    let url = format!("{SERVER}/img/{i}.png");
                    row![
                        Image::network(&url)
                            .size(side, side)
                            .fit(BoxFit::Cover)
                            .placeholder(Skeleton::new().width(side).height(side))
                            .error_placeholder(Icon::new(Icons::BROKEN_IMAGE)),
                        text(format!("Picture {i}")),
                    ]
                    .gap(12.0)
                    .align(Align::Center)
                })
                .width(width - 32.0)
                .height((height - 250.0).max(100.0));
                Box::new(
                    column![
                        row![
                            button("Measure", Msg::Measure),
                            button("Clear cache", Msg::ClearCache),
                        ]
                        .gap(8.0),
                        text(self.cache.clone()).color(theme.muted),
                        list,
                    ]
                    .gap(8.0),
                )
            }
            Page::Notes => {
                let mut items = column![
                    row![
                        TextField::new(self.draft.clone())
                            .placeholder("A note")
                            .on_input(Msg::Draft)
                            .on_submit(Msg::Add),
                        button("Add", Msg::Add),
                        button("Add 100", Msg::AddHundred),
                    ]
                    .gap(8.0),
                    text(format!("{} · {} notes", self.db_status, self.notes.len()))
                        .color(theme.muted),
                ]
                .gap(8.0);
                for note in self.notes.iter().take(12) {
                    let mark = if note.done { "[x]" } else { "[ ]" };
                    items = items.child(button(
                        format!("{mark} {}", note.body),
                        Msg::Toggle(note.id),
                    ));
                }
                Box::new(items)
            }
            Page::SignIn => Box::new(
                column![
                    row![
                        button("Sign in", Msg::SignIn).variant(Variant::Filled),
                        button("Loopback", Msg::SignInLoopback),
                        button("Refresh", Msg::Refresh),
                    ]
                    .gap(8.0),
                    text(self.auth.clone()).wrap(),
                ]
                .gap(12.0),
            ),
        };
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(theme.background)
                .padding(16.0)
                .child(
                    column![
                        text("frus · data").size(14.0).color(theme.muted),
                        tabs,
                        body
                    ]
                    .gap(12.0),
                ),
        )
    }

    fn title(&self) -> String {
        "frus — data".to_string()
    }
}

frus::main!(DataExample::default());
