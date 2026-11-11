// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A SQLite database** in the application's data folder (feature `sqlite`, milestone 795):
//! [`database()`].
//!
//! One connection, owned by a thread of its own that starts when the database is opened and
//! ends when the last [`Database`] handle is dropped; every call is a future that waits for
//! that thread, so a query never stops a frame. SQLite itself comes from `rusqlite` with its
//! `bundled` build: the same SQLite on every platform, with nothing to install.
//!
//! The schema is versioned: [`DatabaseBuilder::migrations`] lists the steps, the database
//! remembers how many it has run (`PRAGMA user_version`), and opening runs the ones it has
//! not, each in a transaction of its own.
//!
//! The Web has no SQLite: [`DatabaseBuilder::open`] answers [`DatabaseError::Unsupported`]
//! there.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// One value of a column or of a parameter: SQLite's five types.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `NULL`.
    Null,
    /// A 64-bit signed integer.
    Integer(i64),
    /// A 64-bit float.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Bytes.
    Blob(Vec<u8>),
}

macro_rules! integer_values {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(value: $t) -> Self {
                Value::Integer(i64::from(value))
            }
        }
    )*};
}
integer_values!(i8, i16, i32, i64, u8, u16, u32, bool);

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Real(value)
    }
}
impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Value::Real(f64::from(value))
    }
}
impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Text(value.to_string())
    }
}
impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Text(value)
    }
}
impl From<&String> for Value {
    fn from(value: &String) -> Self {
        Value::Text(value.clone())
    }
}
impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Value::Blob(value)
    }
}
impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::Blob(value.to_vec())
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map_or(Value::Null, Into::into)
    }
}

/// The values bound to a statement's `?1`, `?2`, … in order. Made with [`params!`](crate::params),
/// an array, a `Vec`, or `()` for none.
///
/// ```
/// use frus_shell::database::Params;
/// let none: Params = ().into();
/// let two: Params = frus_shell::params!["ada", 36].into();
/// let same_type: Params = [1, 2, 3].into();
/// # let _ = (none, two, same_type);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params(pub Vec<Value>);

impl From<()> for Params {
    fn from(_: ()) -> Self {
        Params(Vec::new())
    }
}
impl From<Vec<Value>> for Params {
    fn from(values: Vec<Value>) -> Self {
        Params(values)
    }
}
impl<T: Into<Value>, const N: usize> From<[T; N]> for Params {
    fn from(values: [T; N]) -> Self {
        Params(values.into_iter().map(Into::into).collect())
    }
}

/// **Parameters of mixed types** for a statement, in order: `params!["ada", 36, None::<i64>]`.
///
/// ```
/// let values = frus_shell::params!["ada", 36, 1.5, None::<String>];
/// assert_eq!(values.len(), 4);
/// ```
#[macro_export]
macro_rules! params {
    () => { ::std::vec::Vec::<$crate::database::Value>::new() };
    ($($value:expr),+ $(,)?) => {
        ::std::vec![$($crate::database::Value::from($value)),+]
    };
}

/// Turns a column's [`Value`] into a Rust type: `row.get::<String>("name")`.
pub trait FromValue: Sized {
    /// The value as `Self`, or why not.
    fn from_value(value: &Value) -> Result<Self, String>;
}

impl FromValue for Value {
    fn from_value(value: &Value) -> Result<Self, String> {
        Ok(value.clone())
    }
}

impl FromValue for i64 {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Integer(i) => Ok(*i),
            other => Err(format!("an integer, not {}", kind(other))),
        }
    }
}

macro_rules! narrow_integers {
    ($($t:ty),*) => {$(
        impl FromValue for $t {
            fn from_value(value: &Value) -> Result<Self, String> {
                let wide = i64::from_value(value)?;
                <$t>::try_from(wide).map_err(|_| format!("{wide} does not fit in {}", stringify!($t)))
            }
        }
    )*};
}
narrow_integers!(i8, i16, i32, u8, u16, u32, u64, usize);

impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self, String> {
        Ok(i64::from_value(value)? != 0)
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Real(r) => Ok(*r),
            Value::Integer(i) => Ok(*i as f64),
            other => Err(format!("a number, not {}", kind(other))),
        }
    }
}

impl FromValue for f32 {
    fn from_value(value: &Value) -> Result<Self, String> {
        f64::from_value(value).map(|v| v as f32)
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Text(t) => Ok(t.clone()),
            other => Err(format!("text, not {}", kind(other))),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Blob(b) => Ok(b.clone()),
            Value::Text(t) => Ok(t.as_bytes().to_vec()),
            other => Err(format!("bytes, not {}", kind(other))),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "NULL",
        Value::Integer(_) => "an integer",
        Value::Real(_) => "a real",
        Value::Text(_) => "text",
        Value::Blob(_) => "a blob",
    }
}

/// Which column of a [`Row`]: its position (`0`, `1`, …) or its name.
pub trait ColumnIndex {
    /// The position in `columns`, if there is such a column.
    fn position(&self, columns: &[String]) -> Option<usize>;
    /// How the column is named in an error.
    fn describe(&self) -> String;
}

impl ColumnIndex for usize {
    fn position(&self, columns: &[String]) -> Option<usize> {
        (*self < columns.len()).then_some(*self)
    }
    fn describe(&self) -> String {
        format!("column {self}")
    }
}

impl ColumnIndex for &str {
    fn position(&self, columns: &[String]) -> Option<usize> {
        columns.iter().position(|c| c.eq_ignore_ascii_case(self))
    }
    fn describe(&self) -> String {
        format!("column `{self}`")
    }
}

/// One row of a query's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    columns: Arc<[String]>,
    values: Vec<Value>,
}

impl Row {
    /// A row put together by hand, for tests.
    ///
    /// ```
    /// use frus_shell::database::{Row, Value};
    /// let row = Row::new(vec!["id".into()], vec![Value::Integer(7)]);
    /// assert_eq!(row.get::<i64>("id").unwrap(), 7);
    /// ```
    pub fn new(columns: Vec<String>, values: Vec<Value>) -> Self {
        Self {
            columns: columns.into(),
            values,
        }
    }

    /// The column `index` — a position or a name — as a `T`.
    ///
    /// ```
    /// # use frus_shell::database::{Row, Value};
    /// let row = Row::new(vec!["name".into(), "age".into()], vec!["ada".into(), Value::Integer(36)]);
    /// let name: String = row.get("name").unwrap();
    /// let age: u32 = row.get(1).unwrap();
    /// # assert_eq!((name.as_str(), age), ("ada", 36));
    /// ```
    pub fn get<T: FromValue>(&self, index: impl ColumnIndex) -> Result<T, DatabaseError> {
        let at = index
            .position(&self.columns)
            .ok_or_else(|| DatabaseError::NoColumn(index.describe()))?;
        T::from_value(&self.values[at]).map_err(|why| DatabaseError::Type {
            column: index.describe(),
            message: why,
        })
    }

    /// The columns' names, in order.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The values, in the columns' order.
    pub fn values(&self) -> &[Value] {
        &self.values
    }
}

/// Why a database call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum DatabaseError {
    /// This platform has no SQLite: the Web.
    Unsupported,
    /// The platform names no folder for the application's data.
    NoDataFolder,
    /// SQLite refused: a syntax error, a constraint, a file it cannot open.
    Sqlite(String),
    /// A migration failed; the database is left at the version before it.
    Migration {
        /// The version the failed migration was to bring the database to (the first is 1).
        version: u32,
        /// What SQLite said.
        message: String,
    },
    /// The file was written by a newer version of the application, with more migrations
    /// than this one knows: opening it would risk a schema this one does not understand.
    NewerThanApp {
        /// The version the file is at.
        found: u32,
        /// The migrations this application has.
        known: u32,
    },
    /// No column of that name or position.
    NoColumn(String),
    /// The column holds another type than the one asked for.
    Type {
        /// Which column.
        column: String,
        /// What it held instead.
        message: String,
    },
    /// The database's thread is gone: it panicked, or could not be started.
    Closed,
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatabaseError::Unsupported => write!(f, "SQLite is not available on this platform"),
            DatabaseError::NoDataFolder => {
                write!(
                    f,
                    "this platform names no folder for the application's data"
                )
            }
            DatabaseError::Sqlite(m) => write!(f, "SQLite: {m}"),
            DatabaseError::Migration { version, message } => {
                write!(f, "migration to version {version} failed: {message}")
            }
            DatabaseError::NewerThanApp { found, known } => write!(
                f,
                "the database is at version {found}, newer than the {known} this application knows"
            ),
            DatabaseError::NoColumn(c) => write!(f, "no {c}"),
            DatabaseError::Type { column, message } => write!(f, "{column}: expected {message}"),
            DatabaseError::Closed => write!(f, "the database's thread is gone"),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// **Opens a SQLite database** called `name` in the application's data folder
/// ([`app_data_dir`](crate::app_data_dir)), creating it if it is missing (feature `sqlite`).
///
/// `name` may also be a full path, or `":memory:"` for a database that lives only as long
/// as its handle.
///
/// ```no_run
/// # async fn run() -> Result<(), frus_shell::database::DatabaseError> {
/// use frus_shell::params;
///
/// let db = frus_shell::database("notes.db")
///     .migrations(&[
///         "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
///         "ALTER TABLE notes ADD COLUMN done INTEGER NOT NULL DEFAULT 0",
///     ])
///     .open()
///     .await?;
/// let id = db.insert("INSERT INTO notes (body) VALUES (?1)", params!["milk"]).await?;
/// for row in db.query("SELECT id, body FROM notes WHERE done = ?1", [false]).await? {
///     let body: String = row.get("body")?;
///     println!("{id}: {body}");
/// }
/// # Ok(()) }
/// ```
pub fn database(name: impl Into<String>) -> DatabaseBuilder {
    DatabaseBuilder {
        name: name.into(),
        migrations: Vec::new(),
    }
}

/// A database about to be opened: see [`database()`].
#[derive(Clone, Debug)]
pub struct DatabaseBuilder {
    name: String,
    migrations: Vec<String>,
}

impl DatabaseBuilder {
    /// The schema's steps, oldest first: the first brings an empty database to version 1,
    /// the second to version 2, and so on. Opening runs those the file has not run yet, each
    /// in a transaction of its own, so a step that fails leaves the database at the version
    /// before it. Never edit a step that has shipped: add one.
    ///
    /// ```no_run
    /// let builder = frus_shell::database("app.db").migrations(&[
    ///     "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
    ///     "CREATE INDEX users_name ON users (name)",
    /// ]);
    /// # let _ = builder;
    /// ```
    #[must_use]
    pub fn migrations(mut self, steps: &[&str]) -> Self {
        self.migrations = steps.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Where the file is: `None` for `":memory:"`.
    fn path(&self) -> Result<Option<PathBuf>, DatabaseError> {
        if self.name == ":memory:" {
            return Ok(None);
        }
        let given = PathBuf::from(&self.name);
        if given.is_absolute() {
            return Ok(Some(given));
        }
        let dir = crate::app_data_dir().ok_or(DatabaseError::NoDataFolder)?;
        Ok(Some(dir.join(given)))
    }

    /// Opens the database on a thread of its own, and runs the migrations it has not run.
    ///
    /// ```no_run
    /// # async fn run() -> Result<(), frus_shell::database::DatabaseError> {
    /// let db = frus_shell::database("app.db").open().await?;
    /// println!("schema version {}", db.version().await?);
    /// # Ok(()) }
    /// ```
    pub async fn open(self) -> Result<Database, DatabaseError> {
        #[cfg(not(web))]
        {
            native::open(self.path()?, self.migrations).await
        }
        #[cfg(web)]
        {
            let _ = (self.path(), self.migrations);
            Err(DatabaseError::Unsupported)
        }
    }
}

/// **An open SQLite database** (feature `sqlite`): cheap to clone, every clone the same
/// connection. Made with [`database()`].
///
/// Every call waits for the database's thread, so none of them stops a frame; a
/// [`transaction`](Database::transaction) runs on that thread as a whole.
#[derive(Clone)]
pub struct Database {
    #[cfg(not(web))]
    inner: Arc<native::Inner>,
    #[cfg(web)]
    never: std::convert::Infallible,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Runs one statement, returning how many rows it changed.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// let changed = db.execute("UPDATE notes SET done = 1 WHERE id = ?1", [7]).await?;
    /// # let _ = changed; Ok(()) }
    /// ```
    pub async fn execute(
        &self,
        sql: impl Into<String>,
        params: impl Into<Params>,
    ) -> Result<usize, DatabaseError> {
        let (sql, params) = (sql.into(), params.into());
        self.run(move |tx| tx.execute(&sql, params)).await
    }

    /// Runs an `INSERT`, returning the new row's id (`last_insert_rowid`).
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// let id = db.insert("INSERT INTO notes (body) VALUES (?1)", ["bread"]).await?;
    /// # let _ = id; Ok(()) }
    /// ```
    pub async fn insert(
        &self,
        sql: impl Into<String>,
        params: impl Into<Params>,
    ) -> Result<i64, DatabaseError> {
        let (sql, params) = (sql.into(), params.into());
        self.run(move |tx| tx.insert(&sql, params)).await
    }

    /// Runs a query, returning every row.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// let rows = db.query("SELECT body FROM notes ORDER BY id", ()).await?;
    /// let bodies: Vec<String> = rows.iter().map(|r| r.get(0)).collect::<Result<_, _>>()?;
    /// # let _ = bodies; Ok(()) }
    /// ```
    pub async fn query(
        &self,
        sql: impl Into<String>,
        params: impl Into<Params>,
    ) -> Result<Vec<Row>, DatabaseError> {
        let (sql, params) = (sql.into(), params.into());
        self.run(move |tx| tx.query(&sql, params)).await
    }

    /// Runs a query, returning its first row, if it has one.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// let count: i64 = db
    ///     .query_one("SELECT count(*) FROM notes", ())
    ///     .await?
    ///     .map_or(Ok(0), |row| row.get(0))?;
    /// # let _ = count; Ok(()) }
    /// ```
    pub async fn query_one(
        &self,
        sql: impl Into<String>,
        params: impl Into<Params>,
    ) -> Result<Option<Row>, DatabaseError> {
        let (sql, params) = (sql.into(), params.into());
        self.run(move |tx| tx.query_one(&sql, params)).await
    }

    /// Runs several statements separated by `;`, with no parameters.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// db.execute_batch("DELETE FROM notes; VACUUM").await?;
    /// # Ok(()) }
    /// ```
    pub async fn execute_batch(&self, sql: impl Into<String>) -> Result<(), DatabaseError> {
        let sql = sql.into();
        self.run(move |tx| tx.execute_batch(&sql)).await
    }

    /// **Runs `work` in a transaction**, on the database's thread: committed if it returns
    /// `Ok`, rolled back if it returns `Err` (or panics), so either all of its changes are
    /// kept or none is.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// use frus_shell::params;
    /// db.transaction(|tx| {
    ///     tx.execute("UPDATE accounts SET balance = balance - ?1 WHERE id = ?2", params![10, 1])?;
    ///     tx.execute("UPDATE accounts SET balance = balance + ?1 WHERE id = ?2", params![10, 2])?;
    ///     Ok(())
    /// })
    /// .await?;
    /// # Ok(()) }
    /// ```
    pub async fn transaction<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static,
    ) -> Result<T, DatabaseError> {
        #[cfg(not(web))]
        {
            self.inner.transaction(work).await
        }
        #[cfg(web)]
        {
            let _ = work;
            match self.never {}
        }
    }

    /// The schema's version: how many migrations have run.
    ///
    /// ```no_run
    /// # async fn run(db: frus_shell::database::Database) -> Result<(), frus_shell::database::DatabaseError> {
    /// assert_eq!(db.version().await?, 2);
    /// # Ok(()) }
    /// ```
    pub async fn version(&self) -> Result<u32, DatabaseError> {
        self.run(|tx| {
            tx.query_one("PRAGMA user_version", Params::default())?
                .map_or(Ok(0), |row| row.get(0))
        })
        .await
    }

    /// The file, or `None` for a database in memory.
    ///
    /// ```no_run
    /// # fn run(db: frus_shell::database::Database) {
    /// println!("{:?}", db.path());
    /// # }
    /// ```
    pub fn path(&self) -> Option<&std::path::Path> {
        #[cfg(not(web))]
        {
            self.inner.path.as_deref()
        }
        #[cfg(web)]
        {
            match self.never {}
        }
    }

    /// One statement's work, outside any transaction of the caller's: SQLite's own
    /// statement-level atomicity.
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static,
    ) -> Result<T, DatabaseError> {
        #[cfg(not(web))]
        {
            self.inner.run(work).await
        }
        #[cfg(web)]
        {
            let _ = work;
            match self.never {}
        }
    }
}

/// The connection inside [`Database::transaction`]: the same calls, synchronous, since they
/// already run on the database's thread.
pub struct Transaction<'a> {
    #[cfg(not(web))]
    connection: &'a rusqlite::Connection,
    #[cfg(web)]
    never: (std::convert::Infallible, std::marker::PhantomData<&'a ()>),
}

impl Transaction<'_> {
    /// Runs one statement, returning how many rows it changed.
    pub fn execute(
        &mut self,
        sql: &str,
        params: impl Into<Params>,
    ) -> Result<usize, DatabaseError> {
        #[cfg(not(web))]
        {
            native::execute(self.connection, sql, params.into())
        }
        #[cfg(web)]
        {
            let _ = (sql, params);
            match self.never.0 {}
        }
    }

    /// Runs an `INSERT`, returning the new row's id.
    pub fn insert(&mut self, sql: &str, params: impl Into<Params>) -> Result<i64, DatabaseError> {
        self.execute(sql, params)?;
        #[cfg(not(web))]
        {
            Ok(self.connection.last_insert_rowid())
        }
        #[cfg(web)]
        {
            match self.never.0 {}
        }
    }

    /// Runs a query, returning every row.
    pub fn query(
        &mut self,
        sql: &str,
        params: impl Into<Params>,
    ) -> Result<Vec<Row>, DatabaseError> {
        #[cfg(not(web))]
        {
            native::query(self.connection, sql, params.into(), usize::MAX)
        }
        #[cfg(web)]
        {
            let _ = (sql, params);
            match self.never.0 {}
        }
    }

    /// Runs a query, returning its first row, if it has one.
    pub fn query_one(
        &mut self,
        sql: &str,
        params: impl Into<Params>,
    ) -> Result<Option<Row>, DatabaseError> {
        #[cfg(not(web))]
        {
            Ok(native::query(self.connection, sql, params.into(), 1)?
                .into_iter()
                .next())
        }
        #[cfg(web)]
        {
            let _ = (sql, params);
            match self.never.0 {}
        }
    }

    /// Runs several statements separated by `;`, with no parameters.
    pub fn execute_batch(&mut self, sql: &str) -> Result<(), DatabaseError> {
        #[cfg(not(web))]
        {
            self.connection.execute_batch(sql).map_err(native::sqlite)
        }
        #[cfg(web)]
        {
            let _ = sql;
            match self.never.0 {}
        }
    }
}

#[cfg(not(web))]
mod native {
    use super::*;
    use rusqlite::types::ValueRef;

    /// Work for the database's thread.
    type Job = Box<dyn FnOnce(&mut rusqlite::Connection) + Send + 'static>;

    pub(super) struct Inner {
        jobs: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
        pub(super) path: Option<PathBuf>,
    }

    pub(super) fn sqlite(err: rusqlite::Error) -> DatabaseError {
        DatabaseError::Sqlite(err.to_string())
    }

    fn bind(params: &Params) -> Vec<rusqlite::types::Value> {
        params
            .0
            .iter()
            .map(|value| match value {
                Value::Null => rusqlite::types::Value::Null,
                Value::Integer(i) => rusqlite::types::Value::Integer(*i),
                Value::Real(r) => rusqlite::types::Value::Real(*r),
                Value::Text(t) => rusqlite::types::Value::Text(t.clone()),
                Value::Blob(b) => rusqlite::types::Value::Blob(b.clone()),
            })
            .collect()
    }

    pub(super) fn execute(
        connection: &rusqlite::Connection,
        sql: &str,
        params: Params,
    ) -> Result<usize, DatabaseError> {
        let mut statement = connection.prepare_cached(sql).map_err(sqlite)?;
        statement
            .execute(rusqlite::params_from_iter(bind(&params)))
            .map_err(sqlite)
    }

    pub(super) fn query(
        connection: &rusqlite::Connection,
        sql: &str,
        params: Params,
        limit: usize,
    ) -> Result<Vec<Row>, DatabaseError> {
        let mut statement = connection.prepare_cached(sql).map_err(sqlite)?;
        let columns: Arc<[String]> = statement.column_names().map(str::to_string).collect();
        let mut rows = statement
            .query(rusqlite::params_from_iter(bind(&params)))
            .map_err(sqlite)?;
        let mut out = Vec::new();
        while out.len() < limit {
            let Some(row) = rows.next().map_err(sqlite)? else {
                break;
            };
            let values = (0..columns.len())
                .map(|i| {
                    Ok(match row.get_ref(i).map_err(sqlite)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(i) => Value::Integer(i),
                        ValueRef::Real(r) => Value::Real(r),
                        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
                        ValueRef::Blob(b) => Value::Blob(b.to_vec()),
                    })
                })
                .collect::<Result<Vec<_>, DatabaseError>>()?;
            out.push(Row {
                columns: columns.clone(),
                values,
            });
        }
        Ok(out)
    }

    /// Brings the database to `migrations.len()`, one transaction per step.
    pub(super) fn migrate(
        connection: &mut rusqlite::Connection,
        migrations: &[String],
    ) -> Result<(), DatabaseError> {
        let found: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(sqlite)?;
        let known = migrations.len() as u32;
        if found > known {
            return Err(DatabaseError::NewerThanApp { found, known });
        }
        for (step, sql) in migrations.iter().enumerate().skip(found as usize) {
            let version = step as u32 + 1;
            let failed = |err: rusqlite::Error| DatabaseError::Migration {
                version,
                message: err.to_string(),
            };
            let tx = connection.transaction().map_err(failed)?;
            tx.execute_batch(sql).map_err(failed)?;
            tx.pragma_update(None, "user_version", version)
                .map_err(failed)?;
            tx.commit().map_err(failed)?;
        }
        Ok(())
    }

    fn connect(path: Option<&std::path::Path>) -> Result<rusqlite::Connection, DatabaseError> {
        let connection = match path {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                rusqlite::Connection::open(path)
            }
            None => rusqlite::Connection::open_in_memory(),
        }
        .map_err(sqlite)?;
        // Readers do not wait for a writer, a crash loses at most the last transaction, and
        // the foreign keys declared are enforced, which SQLite does not do unless asked.
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; \
                 PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
            )
            .map_err(sqlite)?;
        Ok(connection)
    }

    pub(super) async fn open(
        path: Option<PathBuf>,
        migrations: Vec<String>,
    ) -> Result<Database, DatabaseError> {
        let (opened, answer) = async_channel::bounded(1);
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let file = path.clone();
        std::thread::Builder::new()
            .name("frus-sqlite".to_string())
            .spawn(move || {
                let mut connection = match connect(file.as_deref()).and_then(|mut c| {
                    migrate(&mut c, &migrations)?;
                    Ok(c)
                }) {
                    Ok(connection) => {
                        let _ = opened.send_blocking(Ok(()));
                        connection
                    }
                    Err(err) => {
                        let _ = opened.send_blocking(Err(err));
                        return;
                    }
                };
                // Until the last handle goes and the channel with it.
                while let Ok(job) = inbox.recv() {
                    job(&mut connection);
                }
            })
            .map_err(|_| DatabaseError::Closed)?;
        answer.recv().await.map_err(|_| DatabaseError::Closed)??;
        Ok(Database {
            inner: Arc::new(Inner {
                jobs: std::sync::Mutex::new(jobs),
                path,
            }),
        })
    }

    impl Inner {
        async fn send<T: Send + 'static>(
            &self,
            job: impl FnOnce(&mut rusqlite::Connection) -> Result<T, DatabaseError> + Send + 'static,
        ) -> Result<T, DatabaseError> {
            let (reply, answer) = async_channel::bounded(1);
            let job: Job = Box::new(move |connection| {
                // A panic in the application's closure must not take the thread, and every
                // later call, with it.
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(connection)))
                        .unwrap_or(Err(DatabaseError::Sqlite(
                            "the work given to the database panicked".to_string(),
                        )));
                let _ = reply.send_blocking(result);
            });
            self.jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .send(job)
                .map_err(|_| DatabaseError::Closed)?;
            answer.recv().await.map_err(|_| DatabaseError::Closed)?
        }

        pub(super) async fn run<T: Send + 'static>(
            &self,
            work: impl FnOnce(&mut Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static,
        ) -> Result<T, DatabaseError> {
            self.send(move |connection| work(&mut Transaction { connection }))
                .await
        }

        pub(super) async fn transaction<T: Send + 'static>(
            &self,
            work: impl FnOnce(&mut Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static,
        ) -> Result<T, DatabaseError> {
            self.send(move |connection| {
                // Dropped without a commit — an `Err`, or a panic unwinding through here —
                // the transaction rolls back.
                let tx = connection.transaction().map_err(sqlite)?;
                let value = work(&mut Transaction { connection: &tx })?;
                tx.commit().map_err(sqlite)?;
                Ok(value)
            })
            .await
        }
    }
}

#[cfg(all(test, not(web)))]
mod tests {
    use super::*;
    use futures_lite::future::block_on;

    fn file(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("frus-db-{name}-{}.db", std::process::id()));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        path.display().to_string()
    }

    const V1: &str = "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)";
    const V2: &str = "ALTER TABLE notes ADD COLUMN done INTEGER NOT NULL DEFAULT 0";

    #[test]
    fn migrations_run_once_each_and_in_order() {
        let path = file("migrate");
        block_on(async {
            let db = database(&path).migrations(&[V1]).open().await.unwrap();
            assert_eq!(db.version().await.unwrap(), 1);
            db.insert("INSERT INTO notes (body) VALUES (?1)", ["milk"])
                .await
                .unwrap();
            drop(db);
            // The next version of the application adds a step: only that one runs, and
            // the row written before is still there.
            let db = database(&path).migrations(&[V1, V2]).open().await.unwrap();
            assert_eq!(db.version().await.unwrap(), 2);
            let row = db
                .query_one("SELECT body, done FROM notes", ())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.get::<String>("body").unwrap(), "milk");
            assert!(!row.get::<bool>("done").unwrap());
            drop(db);
            // An older application meeting the newer file refuses it.
            let older = database(&path).migrations(&[V1]).open().await;
            assert_eq!(
                older.unwrap_err(),
                DatabaseError::NewerThanApp { found: 2, known: 1 }
            );
        });
    }

    #[test]
    fn a_failed_migration_leaves_the_version_before_it() {
        let path = file("failed");
        block_on(async {
            let err = database(&path)
                .migrations(&[V1, "ALTER TABLE nowhere ADD COLUMN x"])
                .open()
                .await
                .unwrap_err();
            assert!(
                matches!(err, DatabaseError::Migration { version: 2, .. }),
                "{err}"
            );
            let db = database(&path).migrations(&[V1]).open().await.unwrap();
            assert_eq!(db.version().await.unwrap(), 1, "step 1 kept, step 2 undone");
        });
    }

    #[test]
    fn typed_parameters_and_rows_round_trip() {
        block_on(async {
            let db = database(":memory:")
                .migrations(&["CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n TEXT)"])
                .open()
                .await
                .unwrap();
            assert!(db.path().is_none());
            db.execute(
                "INSERT INTO t VALUES (?1, ?2, ?3, ?4, ?5)",
                crate::params![42, 1.5, "ada", vec![1u8, 2, 3], None::<String>],
            )
            .await
            .unwrap();
            let rows = db.query("SELECT * FROM t", ()).await.unwrap();
            let row = &rows[0];
            assert_eq!(row.get::<i64>(0).unwrap(), 42);
            assert_eq!(row.get::<u8>("i").unwrap(), 42);
            assert_eq!(row.get::<f64>("r").unwrap(), 1.5);
            assert_eq!(row.get::<String>("s").unwrap(), "ada");
            assert_eq!(row.get::<Vec<u8>>("b").unwrap(), vec![1, 2, 3]);
            assert_eq!(row.get::<Option<String>>("n").unwrap(), None);
            assert!(matches!(
                row.get::<String>("i"),
                Err(DatabaseError::Type { .. })
            ));
            assert!(matches!(
                row.get::<i64>("missing"),
                Err(DatabaseError::NoColumn(_))
            ));
            assert!(matches!(
                db.execute("INSERT INTO nowhere VALUES (1)", ()).await,
                Err(DatabaseError::Sqlite(_))
            ));
        });
    }

    #[test]
    fn a_transaction_keeps_all_or_nothing() {
        block_on(async {
            let db = database(":memory:")
                .migrations(&[
                    "CREATE TABLE accounts (id INTEGER PRIMARY KEY, balance INTEGER NOT NULL CHECK (balance >= 0))",
                    "INSERT INTO accounts VALUES (1, 100), (2, 0)",
                ])
                .open()
                .await
                .unwrap();
            let move_money = |amount: i64| {
                let db = db.clone();
                async move {
                    db.transaction(move |tx| {
                        tx.execute(
                            "UPDATE accounts SET balance = balance + ?1 WHERE id = 2",
                            [amount],
                        )?;
                        tx.execute(
                            "UPDATE accounts SET balance = balance - ?1 WHERE id = 1",
                            [amount],
                        )?;
                        Ok(())
                    })
                    .await
                }
            };
            move_money(30).await.unwrap();
            // Too much: the second update breaks the CHECK, and the first is undone with it.
            assert!(move_money(500).await.is_err());
            let balances: Vec<i64> = db
                .query("SELECT balance FROM accounts ORDER BY id", ())
                .await
                .unwrap()
                .iter()
                .map(|r| r.get(0).unwrap())
                .collect();
            assert_eq!(balances, vec![70, 30]);
            // A panic inside rolls back too, and the database keeps working.
            let panicked = db
                .transaction(|tx| -> Result<(), DatabaseError> {
                    tx.execute("DELETE FROM accounts", ())?;
                    panic!("boom")
                })
                .await;
            assert!(panicked.is_err());
            let count: i64 = db
                .query_one("SELECT count(*) FROM accounts", ())
                .await
                .unwrap()
                .unwrap()
                .get(0)
                .unwrap();
            assert_eq!(count, 2);
        });
    }

    /// The calls are answered off the caller's thread: many in flight at once, all answered.
    #[test]
    fn calls_from_many_tasks_are_serialised_on_the_thread() {
        block_on(async {
            let db = database(":memory:")
                .migrations(&["CREATE TABLE n (v INTEGER)"])
                .open()
                .await
                .unwrap();
            let inserts: Vec<_> = (0..100)
                .map(|i| {
                    let db = db.clone();
                    async move { db.insert("INSERT INTO n VALUES (?1)", [i]).await }
                })
                .collect();
            for insert in inserts {
                insert.await.unwrap();
            }
            let sum: i64 = db
                .query_one("SELECT sum(v) FROM n", ())
                .await
                .unwrap()
                .unwrap()
                .get(0)
                .unwrap();
            assert_eq!(sum, (0..100).sum::<i64>());
        });
    }
}
