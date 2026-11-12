// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Signing in through the system's browser**: OAuth 2.0 and OpenID Connect with PKCE
//! (feature `auth`, milestone 795): [`auth_session`].
//!
//! The authorization code flow as RFC 8252 asks of a native application: the sign-in page
//! opens in the system's browser — never a web view the application could read the password
//! from — and comes back to the application with a code, which is exchanged for tokens with
//! the PKCE verifier (RFC 7636, `S256`) that only this run of the application knows.
//!
//! How the answer comes back depends on the redirect address:
//!
//! - **`http://127.0.0.1/…` or `http://localhost/…`** (the desktops, and Android): a listener
//!   on the loopback interface, on the port the address names or, with none, a free one the
//!   address is rewritten with (RFC 8252 §7.3). The browser is shown a page saying it can be
//!   closed; on Android the page is a Custom Tab, and the application comes back over it.
//! - **A custom scheme, `com.example.app:/oauth`** (Android, and a desktop application that
//!   registered it): the link that opens the application. On Android the page is a Custom Tab
//!   and the browser hands the link back with `FLAG_ACTIVITY_CLEAR_TOP`, which reaches the
//!   running activity (frus's own, milestone 801) and closes the tab. The application declares
//!   the scheme (`frus create --deeplink`, or an intent filter); a package built by hand with
//!   `cargo apk` runs in the native activity, which cannot take it
//!   ([`AuthError::Unsupported`]).
//! - **The Web**: a popup on the sign-in page, read once it is back on the application's own
//!   origin; the redirect address is a page of this origin.
//!
//! The `state` sent is checked on the way back, and with an `openid` scope a `nonce` is sent
//! and checked in the ID token.

use std::fmt;
use std::time::Duration;

use crate::FetchError;

/// How long the person has to sign in before [`AuthError::Timeout`]: five minutes.
pub const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Why a sign-in did not end with tokens.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthError {
    /// This platform cannot run the flow, or the redirect address cannot come back to it
    /// here (a loopback address on the Web, a custom scheme in an Android package without
    /// frus's activity).
    Unsupported(String),
    /// The request is not one that can be sent: an address that is not one, no token
    /// endpoint.
    InvalidRequest(String),
    /// The browser could not be opened.
    Browser(String),
    /// Another sign-in is already waiting for its answer.
    Busy,
    /// The person closed the sign-in window.
    Cancelled,
    /// No answer before the deadline ([`AuthRequest::timeout`]).
    Timeout,
    /// The server answered with an error: `access_denied` when the person said no.
    Denied {
        /// The `error` code (RFC 6749 §4.1.2.1).
        error: String,
        /// The `error_description`, if the server gave one.
        description: Option<String>,
    },
    /// The answer did not carry the `state` this request sent: it is not this request's,
    /// and is refused (a forged redirect).
    StateMismatch,
    /// The ID token's `nonce` is not the one this request sent.
    NonceMismatch,
    /// The code could not be exchanged for tokens.
    Token(String),
    /// The loopback listener could not be opened.
    Listener(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::Unsupported(why) => write!(f, "sign-in is not available here: {why}"),
            AuthError::InvalidRequest(why) => write!(f, "invalid sign-in request: {why}"),
            AuthError::Browser(why) => write!(f, "the browser could not be opened: {why}"),
            AuthError::Busy => write!(f, "another sign-in is in progress"),
            AuthError::Cancelled => write!(f, "the sign-in was cancelled"),
            AuthError::Timeout => write!(f, "the sign-in timed out"),
            AuthError::Denied { error, description } => match description {
                Some(d) => write!(f, "the server refused: {error} ({d})"),
                None => write!(f, "the server refused: {error}"),
            },
            AuthError::StateMismatch => write!(f, "the answer is not this sign-in's (state)"),
            AuthError::NonceMismatch => write!(f, "the ID token is not this sign-in's (nonce)"),
            AuthError::Token(why) => write!(f, "the code could not be exchanged: {why}"),
            AuthError::Listener(why) => write!(f, "the loopback listener failed: {why}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// What the token endpoint answered (RFC 6749 §5.1, and OpenID Connect's `id_token`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthTokens {
    /// The token to send as `Authorization: Bearer …`.
    pub access_token: String,
    /// Its type, `Bearer` almost always.
    pub token_type: String,
    /// How many seconds the access token lasts, if the server said.
    pub expires_in: Option<u64>,
    /// The token that gets new access tokens ([`AuthRequest::refresh`]), if one was given.
    pub refresh_token: Option<String>,
    /// The OpenID Connect ID token (a JWT), with an `openid` scope.
    pub id_token: Option<String>,
    /// The scopes granted, when they differ from those asked for.
    pub scope: Option<String>,
}

/// **A sign-in to run**: where the authorization server is, who the application is to it,
/// and where the answer comes back. Run it with [`auth_session`].
///
/// ```no_run
/// use frus_shell::{auth_session, AuthRequest};
/// # async fn sign_in() -> Result<(), frus_shell::AuthError> {
/// let tokens = auth_session(
///     AuthRequest::new(
///         "https://accounts.example.com/authorize",
///         "my-client-id",
///         "http://127.0.0.1/callback",
///     )
///     .token_url("https://accounts.example.com/token")
///     .scope("openid profile offline_access"),
/// )
/// .await?;
/// println!("signed in: {}", tokens.access_token);
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct AuthRequest {
    authorize_url: String,
    token_url: Option<String>,
    client_id: String,
    redirect_uri: String,
    scopes: Vec<String>,
    extra: Vec<(String, String)>,
    timeout: Duration,
    success_page: Option<String>,
    #[cfg(feature = "secure-store")]
    refresh_key: Option<String>,
}

impl AuthRequest {
    /// A request to the authorization endpoint `authorize_url`, for the public client
    /// `client_id`, answered at `redirect_uri`. Add [`token_url`](Self::token_url).
    pub fn new(
        authorize_url: impl Into<String>,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            authorize_url: authorize_url.into(),
            token_url: None,
            client_id: client_id.into(),
            redirect_uri: redirect_uri.into(),
            scopes: Vec::new(),
            extra: Vec::new(),
            timeout: DEFAULT_AUTH_TIMEOUT,
            success_page: None,
            #[cfg(feature = "secure-store")]
            refresh_key: None,
        }
    }

    /// The token endpoint the code is exchanged at. Required.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .token_url("https://a.example/token");
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn token_url(mut self, url: impl Into<String>) -> Self {
        self.token_url = Some(url.into());
        self
    }

    /// Scopes to ask for, separated by spaces; they add up over calls. An `openid` scope also
    /// sends a `nonce`, checked in the ID token.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .scope("openid email")
    ///     .scope("offline_access");
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn scope(mut self, scopes: &str) -> Self {
        self.scopes
            .extend(scopes.split_whitespace().map(str::to_string));
        self
    }

    /// Another parameter for the authorization endpoint: `prompt=login`, `login_hint=…`,
    /// `audience=…`.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .param("prompt", "login");
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push((name.into(), value.into()));
        self
    }

    /// How long the person has to sign in; [`DEFAULT_AUTH_TIMEOUT`] otherwise.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .timeout(std::time::Duration::from_secs(120));
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The HTML the browser shows once a loopback redirect is received; a short page saying
    /// the window can be closed otherwise.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .success_page("<h1>Welcome back</h1><p>Return to the app.</p>");
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn success_page(mut self, html: impl Into<String>) -> Self {
        self.success_page = Some(html.into());
        self
    }

    /// Keeps the refresh token the sign-in gets in the [`secure_store`](crate::secure_store)
    /// under `key` (feature `secure-store`), and [`stored_refresh_token`] reads it back.
    ///
    /// ```
    /// let request = frus_shell::AuthRequest::new("https://a.example/auth", "id", "http://127.0.0.1/cb")
    ///     .store_refresh_token("example-refresh");
    /// # let _ = request;
    /// ```
    #[cfg(feature = "secure-store")]
    #[must_use]
    pub fn store_refresh_token(mut self, key: impl Into<String>) -> Self {
        self.refresh_key = Some(key.into());
        self
    }

    /// **New tokens for a refresh token** (RFC 6749 §6), from the same token endpoint: what
    /// keeps a person signed in across launches without the browser.
    ///
    /// ```no_run
    /// # async fn run(request: frus_shell::AuthRequest, refresh: String) -> Result<(), frus_shell::AuthError> {
    /// let tokens = request.refresh(&refresh).await?;
    /// # let _ = tokens; Ok(()) }
    /// ```
    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokens, AuthError> {
        let token_url = self
            .token_url
            .as_deref()
            .ok_or_else(|| AuthError::InvalidRequest("no token endpoint".to_string()))?;
        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.to_string()),
            ("client_id", self.client_id.clone()),
        ];
        if !self.scopes.is_empty() {
            form.push(("scope", self.scopes.join(" ")));
        }
        let tokens = post_form(token_url, &form).await?;
        self.keep(&tokens);
        Ok(tokens)
    }

    #[cfg(feature = "secure-store")]
    fn keep(&self, tokens: &AuthTokens) {
        if let (Some(key), Some(refresh)) = (&self.refresh_key, &tokens.refresh_token) {
            if let Err(err) = crate::secure_store().set(key, refresh) {
                log::warn!("frus: the refresh token could not be kept: {err}");
            }
        }
    }

    #[cfg(not(feature = "secure-store"))]
    fn keep(&self, _tokens: &AuthTokens) {}
}

/// The refresh token a sign-in kept under `key` ([`AuthRequest::store_refresh_token`]).
///
/// ```no_run
/// if let Some(refresh) = frus_shell::stored_refresh_token("example-refresh") {
///     # let _ = refresh;
///     // request.refresh(&refresh).await
/// }
/// ```
#[cfg(feature = "secure-store")]
pub fn stored_refresh_token(key: &str) -> Option<String> {
    crate::secure_store().get(key).ok().flatten()
}

/// What one run of the flow sends, made once so that the checks on the way back compare
/// with exactly what went out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub url: String,
    pub state: String,
    pub verifier: String,
    pub nonce: Option<String>,
    pub redirect_uri: String,
}

/// The authorization address for `request`, answered at `redirect_uri`, with the secrets
/// given (so that the tests can give them).
pub(crate) fn prepare(
    request: &AuthRequest,
    redirect_uri: &str,
    state: String,
    verifier: String,
    nonce: Option<String>,
    challenge: &str,
) -> Prepared {
    let mut query = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), request.client_id.clone()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("state".to_string(), state.clone()),
        ("code_challenge".to_string(), challenge.to_string()),
        ("code_challenge_method".to_string(), "S256".to_string()),
    ];
    if !request.scopes.is_empty() {
        query.push(("scope".to_string(), request.scopes.join(" ")));
    }
    if let Some(nonce) = &nonce {
        query.push(("nonce".to_string(), nonce.clone()));
    }
    query.extend(request.extra.iter().cloned());
    let encoded = form_encode(query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let joiner = if request.authorize_url.contains('?') {
        '&'
    } else {
        '?'
    };
    Prepared {
        url: format!("{}{joiner}{encoded}", request.authorize_url),
        state,
        verifier,
        nonce,
        redirect_uri: redirect_uri.to_string(),
    }
}

/// `application/x-www-form-urlencoded`, the unreserved characters kept as they are.
pub(crate) fn form_encode<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    pairs
        .into_iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The parameters of an address's query (and of its fragment, where an implicit answer
/// would put them), decoded.
pub(crate) fn query_of(url: &str) -> Vec<(String, String)> {
    let after = match url.split_once('?') {
        Some((_, after)) => after,
        None => match url.split_once('#') {
            Some((_, after)) => after,
            None => return Vec::new(),
        },
    };
    after
        .split(['&', '#'])
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// The code an answer carries, once its `state` is checked, or why there is none.
pub(crate) fn code_from(answer: &str, prepared: &Prepared) -> Result<String, AuthError> {
    let params = query_of(answer);
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    // The state first: an error that is not this request's is not reported as one.
    if get("state").as_deref() != Some(prepared.state.as_str()) {
        return Err(AuthError::StateMismatch);
    }
    if let Some(error) = get("error") {
        return Err(AuthError::Denied {
            error,
            description: get("error_description"),
        });
    }
    get("code")
        .filter(|c| !c.is_empty())
        .ok_or_else(|| AuthError::Token("the answer carries no code".to_string()))
}

/// Base64url without padding (RFC 4648 §5), what PKCE and JWTs use.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Base64url read back, padding or not; `None` for anything else.
pub(crate) fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' | b'+' => Some(62),
        b'_' | b'/' => Some(63),
        _ => None,
    };
    let text = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= u32::from(value(*c)?) << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}

/// The `nonce` claim of a JWT's payload, if it has one. The signature is not checked: the
/// token came straight from the token endpoint over TLS, which OpenID Connect accepts in its
/// place (Core §3.1.3.7).
pub(crate) fn nonce_of(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let json: serde_json::Value = serde_json::from_slice(&base64url_decode(payload)?).ok()?;
    json.get("nonce")?.as_str().map(str::to_string)
}

/// The tokens in a token endpoint's JSON answer.
pub(crate) fn tokens_from(body: &str) -> Result<AuthTokens, AuthError> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AuthError::Token(e.to_string()))?;
    let text = |name: &str| json.get(name).and_then(|v| v.as_str()).map(str::to_string);
    if let Some(error) = text("error") {
        return Err(AuthError::Denied {
            error,
            description: text("error_description"),
        });
    }
    Ok(AuthTokens {
        access_token: text("access_token")
            .ok_or_else(|| AuthError::Token("no access_token in the answer".to_string()))?,
        token_type: text("token_type").unwrap_or_else(|| "Bearer".to_string()),
        expires_in: json.get("expires_in").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        }),
        refresh_token: text("refresh_token"),
        id_token: text("id_token"),
        scope: text("scope"),
    })
}

/// POSTs `form` to the token endpoint and reads the tokens.
async fn post_form(url: &str, form: &[(&str, String)]) -> Result<AuthTokens, AuthError> {
    let body = form_encode(form.iter().map(|(k, v)| (*k, v.as_str())));
    let answer = crate::Request::post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send_response()
        .await;
    match answer {
        Ok(response) => tokens_from(&String::from_utf8_lossy(response.body())),
        // An error status may still carry the server's JSON reason; the transport keeps
        // only the code, which is what is said.
        Err(FetchError::Status(code)) => Err(AuthError::Token(format!("HTTP status {code}"))),
        Err(err) => Err(AuthError::Token(err.to_string())),
    }
}

/// `count` random bytes from the system's generator.
fn random(count: usize) -> Result<Vec<u8>, AuthError> {
    let mut bytes = vec![0u8; count];
    #[cfg(not(web))]
    {
        use ring::rand::SecureRandom;
        ring::rand::SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| AuthError::Unsupported("no random generator".to_string()))?;
    }
    #[cfg(web)]
    {
        web_sys::window()
            .and_then(|w| w.crypto().ok())
            .and_then(|c| c.get_random_values_with_u8_array(&mut bytes).ok())
            .ok_or_else(|| AuthError::Unsupported("no random generator".to_string()))?;
    }
    Ok(bytes)
}

/// The PKCE challenge of `verifier`: base64url(SHA-256(verifier)).
#[cfg(not(web))]
pub(crate) async fn challenge_of(verifier: &str) -> Result<String, AuthError> {
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    Ok(base64url(digest.as_ref()))
}

/// The PKCE challenge of `verifier`, through the browser's `crypto.subtle`.
#[cfg(web)]
pub(crate) async fn challenge_of(verifier: &str) -> Result<String, AuthError> {
    let unsupported = |_| AuthError::Unsupported("crypto.subtle (an https page)".to_string());
    let subtle = web_sys::window()
        .and_then(|w| w.crypto().ok())
        .ok_or_else(|| AuthError::Unsupported("no crypto".to_string()))?
        .subtle();
    let data = js_sys::Uint8Array::from(verifier.as_bytes());
    let promise = subtle
        .digest_with_str_and_buffer_source("SHA-256", &data)
        .map_err(unsupported)?;
    let buffer = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(unsupported)?;
    Ok(base64url(&js_sys::Uint8Array::new(&buffer).to_vec()))
}

/// **Runs a sign-in**: opens the authorization page in the system's browser (a Custom Tab for
/// a loopback address on Android, a popup on the Web), waits for the redirect, checks its `state`, and exchanges
/// the code for tokens with the PKCE verifier (feature `auth`).
///
/// ```no_run
/// # async fn sign_in() -> Result<(), frus_shell::AuthError> {
/// let request = frus_shell::AuthRequest::new(
///     "https://accounts.example.com/authorize",
///     "my-client-id",
///     "com.example.notes:/oauth", // the scheme `frus create --deeplink` registered
/// )
/// .token_url("https://accounts.example.com/token")
/// .scope("openid offline_access");
/// let tokens = frus_shell::auth_session(request).await?;
/// # let _ = tokens; Ok(()) }
/// ```
///
/// One sign-in at a time: a second while the first waits is [`AuthError::Busy`].
pub async fn auth_session(request: AuthRequest) -> Result<AuthTokens, AuthError> {
    let token_url = request
        .token_url
        .clone()
        .ok_or_else(|| AuthError::InvalidRequest("no token endpoint (token_url)".to_string()))?;
    let state = base64url(&random(16)?);
    let verifier = base64url(&random(32)?);
    let nonce = request
        .scopes
        .iter()
        .any(|s| s == "openid")
        .then(|| random(16).map(|b| base64url(&b)))
        .transpose()?;
    let challenge = challenge_of(&verifier).await?;
    let redirect = Redirect::of(&request.redirect_uri)?;
    let (answer, prepared) = redirect
        .wait(&request, |redirect_uri| {
            prepare(
                &request,
                redirect_uri,
                state.clone(),
                verifier.clone(),
                nonce.clone(),
                &challenge,
            )
        })
        .await?;
    let code = code_from(&answer, &prepared)?;
    let form = [
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", prepared.redirect_uri.clone()),
        ("client_id", request.client_id.clone()),
        ("code_verifier", prepared.verifier.clone()),
    ];
    let tokens = post_form(&token_url, &form).await?;
    if let (Some(sent), Some(id_token)) = (&prepared.nonce, &tokens.id_token) {
        if nonce_of(id_token).as_deref() != Some(sent.as_str()) {
            return Err(AuthError::NonceMismatch);
        }
    }
    request.keep(&tokens);
    Ok(tokens)
}

/// How the answer of a sign-in comes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Redirect {
    /// `http://127.0.0.1:port/path`: a listener on the loopback interface. `port` 0 is any.
    Loopback {
        host: String,
        port: u16,
        path: String,
    },
    /// Any other scheme, or an `https` link the application claims: the link that opens it.
    Link(String),
}

impl Redirect {
    pub(crate) fn of(uri: &str) -> Result<Self, AuthError> {
        let invalid = || AuthError::InvalidRequest(format!("{uri:?} is not a redirect address"));
        let scheme = crate::open_url::scheme_of(uri).ok_or_else(invalid)?;
        if !scheme.eq_ignore_ascii_case("http") {
            return Ok(Redirect::Link(uri.to_string()));
        }
        let rest = uri[scheme.len() + 1..]
            .strip_prefix("//")
            .ok_or_else(invalid)?;
        let (authority, path) = match rest.find(['/', '?', '#']) {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
                (host, port.parse::<u16>().map_err(|_| invalid())?)
            }
            _ => (authority, 0),
        };
        let path = path.split(['?', '#']).next().unwrap_or("/");
        let path = if path.is_empty() { "/" } else { path };
        match host {
            "127.0.0.1" | "localhost" | "[::1]" => Ok(Redirect::Loopback {
                host: host.to_string(),
                port,
                path: path.to_string(),
            }),
            // A web page of the application's own, as the Web's popup uses.
            _ => Ok(Redirect::Link(uri.to_string())),
        }
    }

    /// Opens the page made by `prepare` for the redirect address actually used, and waits
    /// for the answer: the address the browser was sent back to.
    async fn wait(
        &self,
        request: &AuthRequest,
        prepare: impl FnOnce(&str) -> Prepared,
    ) -> Result<(String, Prepared), AuthError> {
        match self {
            #[cfg(any(desktop, android))]
            Redirect::Loopback { host, port, path } => {
                loopback::wait(request, host, *port, path, prepare).await
            }
            #[cfg(not(any(desktop, android)))]
            Redirect::Loopback { .. } => {
                let _ = (request, prepare);
                Err(AuthError::Unsupported(
                    "a loopback redirect needs a desktop; use the application's link".to_string(),
                ))
            }
            // Chrome sends the link back with `FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_CLEAR_TOP`
            // (from a Custom Tab and from a tab alike, measured on Android 10). frus's activity,
            // `singleTop`, takes it as a new intent and hands it here (milestone 801); the
            // native activity is finished and made again, and the application with it, so a
            // package that still names it is told so rather than left waiting.
            #[cfg(android)]
            Redirect::Link(uri) => {
                if crate::android_link::receives_new_intents() {
                    link::wait(request, uri, prepare).await
                } else {
                    let _ = (request, prepare);
                    Err(AuthError::Unsupported(
                        "a custom-scheme redirect needs frus's Android activity, which \
                         `frus build` and `frus run` put in the package; or use \
                         http://127.0.0.1/… (loopback)"
                            .to_string(),
                    ))
                }
            }
            #[cfg(not(any(web, android)))]
            Redirect::Link(uri) => link::wait(request, uri, prepare).await,
            #[cfg(web)]
            Redirect::Link(uri) => popup::wait(request, uri, prepare).await,
        }
    }
}

/// The sign-in waiting for its link, if one is.
#[cfg(not(web))]
struct Waiting {
    /// The redirect address without its query: what a link must start with to be the answer.
    prefix: String,
    answer: async_channel::Sender<String>,
}

#[cfg(not(web))]
static WAITING: std::sync::Mutex<Option<Waiting>> = std::sync::Mutex::new(None);

/// Whether `link` is an answer at the redirect address `prefix`: the same address, then
/// its query or fragment.
pub(crate) fn answers(link: &str, prefix: &str) -> bool {
    let link = link.trim();
    link.len() >= prefix.len()
        && link[..prefix.len()].eq_ignore_ascii_case(prefix)
        && matches!(link[prefix.len()..].chars().next(), None | Some('?' | '#'))
}

/// Hands `link` to the sign-in waiting for it: `true` when it was its answer, and is then
/// not opened as a location.
#[cfg(not(web))]
pub(crate) fn offer(link: &str) -> bool {
    let mut guard = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(waiting) if answers(link, &waiting.prefix) => {
            let _ = waiting.answer.try_send(link.to_string());
            *guard = None;
            true
        }
        _ => false,
    }
}

#[cfg(not(web))]
mod link {
    use super::*;

    /// Removes the waiting sign-in when the future ends, however it ends.
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            *WAITING.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    pub(super) async fn wait(
        request: &AuthRequest,
        uri: &str,
        prepare: impl FnOnce(&str) -> Prepared,
    ) -> Result<(String, Prepared), AuthError> {
        let (sender, receiver) = async_channel::bounded(1);
        {
            let mut guard = WAITING.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_some() {
                return Err(AuthError::Busy);
            }
            *guard = Some(Waiting {
                prefix: uri.split(['?', '#']).next().unwrap_or(uri).to_string(),
                answer: sender,
            });
        }
        let _clear = Clear;
        let prepared = prepare(uri);
        open_page(&prepared.url)?;
        let timeout = async {
            async_io::Timer::after(request.timeout).await;
            Err(AuthError::Timeout)
        };
        let answer = async { receiver.recv().await.map_err(|_| AuthError::Cancelled) };
        let link = futures_lite::future::or(answer, timeout).await?;
        Ok((link, prepared))
    }

    /// The authorization page, in the system's browser — on Android in a Custom Tab over the
    /// application, which the redirect's link closes as it hands the answer back.
    fn open_page(url: &str) -> Result<(), AuthError> {
        #[cfg(android)]
        let opened = crate::android_share::open_auth_tab(url);
        #[cfg(not(android))]
        let opened = crate::open_url(url);
        opened.map_err(|e| AuthError::Browser(e.to_string()))
    }
}

#[cfg(any(desktop, android))]
mod loopback {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    /// What the browser shows once it has handed the answer over.
    const DONE: &str = "<!doctype html><meta charset=utf-8><title>Signed in</title>\
        <body style=\"font-family:system-ui,sans-serif;text-align:center;padding-top:4em\">\
        <h1>You can close this window</h1><p>Return to the application.</p></body>";

    pub(super) async fn wait(
        request: &AuthRequest,
        host: &str,
        port: u16,
        path: &str,
        prepare: impl FnOnce(&str) -> Prepared,
    ) -> Result<(String, Prepared), AuthError> {
        let bind = if host == "[::1]" {
            "[::1]"
        } else {
            "127.0.0.1"
        };
        let listener = TcpListener::bind(format!("{bind}:{port}"))
            .map_err(|e| AuthError::Listener(e.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|e| AuthError::Listener(e.to_string()))?
            .port();
        let redirect_uri = format!("http://{host}:{port}{path}");
        let prepared = prepare(&redirect_uri);
        // Android: a Custom Tab over the application, which comes back over it once the
        // answer is in (the listener is the application's own, so nothing else brings it).
        #[cfg(android)]
        crate::android_share::open_auth_tab(&prepared.url)
            .map_err(|e| AuthError::Browser(e.to_string()))?;
        #[cfg(not(android))]
        crate::open_url(&prepared.url).map_err(|e| AuthError::Browser(e.to_string()))?;
        let (deadline, path) = (
            std::time::Instant::now() + request.timeout,
            path.to_string(),
        );
        let page = request
            .success_page
            .clone()
            .unwrap_or_else(|| DONE.to_string());
        let answer = blocking::unblock(move || accept(listener, &path, &page, deadline)).await;
        #[cfg(android)]
        crate::android_share::return_to_app();
        let answer = answer?;
        Ok((format!("http://{host}:{port}{answer}"), prepared))
    }

    /// Answers the browser's requests until one comes to `path`, whose target is returned.
    fn accept(
        listener: TcpListener,
        path: &str,
        page: &str,
        deadline: std::time::Instant,
    ) -> Result<String, AuthError> {
        listener
            .set_nonblocking(true)
            .map_err(|e| AuthError::Listener(e.to_string()))?;
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(AuthError::Timeout);
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(target) = serve(stream, path, page) {
                        return Ok(target);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(AuthError::Listener(e.to_string())),
            }
        }
    }

    /// Reads one request; the target when it is the redirect, `None` for anything else (a
    /// browser asks for `/favicon.ico` too).
    fn serve(mut stream: TcpStream, path: &str, page: &str) -> Option<String> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buffer = vec![0u8; 8192];
        let mut read = 0;
        while read < buffer.len() {
            match stream.read(&mut buffer[read..]) {
                Ok(0) => break,
                Ok(n) => {
                    read += n;
                    if buffer[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .map(str::to_string);
        let ours = target
            .as_deref()
            .is_some_and(|t| t.split(['?', '#']).next() == Some(path));
        let (status, body) = if ours {
            ("200 OK", page)
        } else {
            ("404 Not Found", "")
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.flush();
        if ours {
            target
        } else {
            None
        }
    }
}

#[cfg(web)]
mod popup {
    use super::*;
    use wasm_bindgen::JsCast;

    /// Waits `ms` on the browser's clock.
    async fn sleep(ms: i32) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    resolve.unchecked_ref(),
                    ms,
                );
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }

    pub(super) async fn wait(
        request: &AuthRequest,
        uri: &str,
        prepare: impl FnOnce(&str) -> Prepared,
    ) -> Result<(String, Prepared), AuthError> {
        let window = web_sys::window().ok_or_else(|| AuthError::Unsupported("no window".into()))?;
        let origin = window
            .location()
            .origin()
            .map_err(|_| AuthError::Unsupported("no origin".into()))?;
        if !uri.starts_with(&origin) {
            return Err(AuthError::Unsupported(format!(
                "the redirect address must be a page of this origin ({origin})"
            )));
        }
        let prepared = prepare(uri);
        let popup = window
            .open_with_url_and_target_and_features(
                &prepared.url,
                "frus-auth",
                "popup,width=520,height=720",
            )
            .map_err(|e| AuthError::Browser(format!("{e:?}")))?
            .ok_or_else(|| AuthError::Browser("the browser blocked the popup".to_string()))?;
        let prefix = uri.split(['?', '#']).next().unwrap_or(uri).to_string();
        let started = js_sys::Date::now();
        loop {
            sleep(200).await;
            if popup.closed().unwrap_or(true) {
                return Err(AuthError::Cancelled);
            }
            // Another origin's address cannot be read: the sign-in is still going on.
            if let Ok(href) = popup.location().href() {
                if answers(&href, &prefix) {
                    let _ = popup.close();
                    return Ok((href, prepared));
                }
            }
            if js_sys::Date::now() - started > request.timeout.as_millis() as f64 {
                let _ = popup.close();
                return Err(AuthError::Timeout);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> AuthRequest {
        AuthRequest::new(
            "https://auth.example/authorize",
            "client 1",
            "http://127.0.0.1/cb",
        )
        .token_url("https://auth.example/token")
        .scope("openid profile")
        .param("prompt", "login")
    }

    /// RFC 7636 appendix B: the verifier and challenge it publishes.
    #[test]
    fn the_pkce_challenge_is_the_rfcs() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = futures_lite::future::block_on(challenge_of(verifier)).unwrap();
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(
            base64url(&[3, 236, 255, 224, 193]),
            "A-z_4ME",
            "the RFC's base64url example"
        );
    }

    #[test]
    fn base64url_round_trips_every_length() {
        for len in 0..40 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64url(&bytes);
            assert!(!text.contains(['=', '+', '/']));
            assert_eq!(base64url_decode(&text), Some(bytes));
        }
        assert_eq!(base64url_decode("a"), None);
        assert_eq!(base64url_decode("@@@@"), None);
    }

    #[test]
    fn random_secrets_are_long_and_differ() {
        let a = base64url(&random(32).unwrap());
        let b = base64url(&random(32).unwrap());
        assert_eq!(a.len(), 43, "RFC 7636: 43 to 128 characters");
        assert_ne!(a, b);
    }

    #[test]
    fn the_authorization_address_carries_everything() {
        let p = prepare(
            &request(),
            "http://127.0.0.1:5555/cb",
            "st".into(),
            "ver".into(),
            Some("nn".into()),
            "chal",
        );
        assert!(p
            .url
            .starts_with("https://auth.example/authorize?response_type=code&"));
        let q = query_of(&p.url);
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("client_id"), Some("client 1"));
        assert!(p.url.contains("client_id=client%201"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:5555/cb"));
        assert!(p
            .url
            .contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555%2Fcb"));
        assert_eq!(get("state"), Some("st"));
        assert_eq!(get("code_challenge"), Some("chal"));
        assert_eq!(get("code_challenge_method"), Some("S256"));
        assert_eq!(get("scope"), Some("openid profile"));
        assert_eq!(get("nonce"), Some("nn"));
        assert_eq!(get("prompt"), Some("login"));
        // An endpoint that already has a query keeps it.
        let other = AuthRequest::new("https://a.example/auth?tenant=x", "c", "app:/cb");
        let p = prepare(&other, "app:/cb", "s".into(), "v".into(), None, "c");
        assert!(p
            .url
            .starts_with("https://a.example/auth?tenant=x&response_type=code"));
        assert!(!p.url.contains("nonce"));
    }

    fn prepared() -> Prepared {
        prepare(&request(), "app:/cb", "good".into(), "v".into(), None, "c")
    }

    #[test]
    fn the_state_is_checked_before_anything_else() {
        assert_eq!(
            code_from("app:/cb?code=abc&state=good", &prepared()),
            Ok("abc".to_string())
        );
        assert_eq!(
            code_from("app:/cb?code=abc&state=evil", &prepared()),
            Err(AuthError::StateMismatch)
        );
        assert_eq!(
            code_from("app:/cb?code=abc", &prepared()),
            Err(AuthError::StateMismatch)
        );
        assert_eq!(
            code_from("app:/cb?error=access_denied&state=evil", &prepared()),
            Err(AuthError::StateMismatch),
            "a forged error is not reported as the server's"
        );
        assert_eq!(
            code_from(
                "app:/cb?error=access_denied&error_description=No+thanks&state=good",
                &prepared()
            ),
            Err(AuthError::Denied {
                error: "access_denied".into(),
                description: Some("No thanks".into())
            })
        );
        assert!(code_from("app:/cb?state=good", &prepared()).is_err());
    }

    #[test]
    fn redirect_addresses_are_told_apart() {
        assert_eq!(
            Redirect::of("http://127.0.0.1/cb").unwrap(),
            Redirect::Loopback {
                host: "127.0.0.1".into(),
                port: 0,
                path: "/cb".into()
            }
        );
        assert_eq!(
            Redirect::of("http://localhost:8400").unwrap(),
            Redirect::Loopback {
                host: "localhost".into(),
                port: 8400,
                path: "/".into()
            }
        );
        assert_eq!(
            Redirect::of("com.example.app:/oauth").unwrap(),
            Redirect::Link("com.example.app:/oauth".into())
        );
        assert_eq!(
            Redirect::of("https://app.example/callback").unwrap(),
            Redirect::Link("https://app.example/callback".into())
        );
        assert!(Redirect::of("nothing").is_err());
        assert!(Redirect::of("http://127.0.0.1:notaport/").is_err());
    }

    #[test]
    fn only_the_redirect_address_is_an_answer() {
        assert!(answers("app:/cb?code=1", "app:/cb"));
        assert!(answers("APP:/cb#code=1", "app:/cb"));
        assert!(answers("app:/cb", "app:/cb"));
        assert!(!answers("app:/cb2?code=1", "app:/cb"));
        assert!(!answers("app:/c", "app:/cb"));
        assert!(!answers("other:/cb?code=1", "app:/cb"));
    }

    #[cfg(not(web))]
    #[test]
    fn a_waiting_sign_in_takes_its_link_and_no_other() {
        let (sender, receiver) = async_channel::bounded(1);
        *WAITING.lock().unwrap() = Some(Waiting {
            prefix: "app:/cb".into(),
            answer: sender,
        });
        assert!(!offer("app:/orders/42"), "a location is left to the router");
        assert!(offer("app:/cb?code=1&state=s"));
        assert_eq!(receiver.try_recv().unwrap(), "app:/cb?code=1&state=s");
        assert!(!offer("app:/cb?code=2"), "answered once");
    }

    #[test]
    fn token_answers_and_their_errors() {
        let t = tokens_from(
            r#"{"access_token":"at","token_type":"bearer","expires_in":3600,"refresh_token":"rt","id_token":"x.y.z"}"#,
        )
        .unwrap();
        assert_eq!(t.access_token, "at");
        assert_eq!(t.expires_in, Some(3600));
        assert_eq!(t.refresh_token.as_deref(), Some("rt"));
        assert_eq!(
            tokens_from(r#"{"error":"invalid_grant","error_description":"bad verifier"}"#),
            Err(AuthError::Denied {
                error: "invalid_grant".into(),
                description: Some("bad verifier".into())
            })
        );
        assert!(tokens_from("not json").is_err());
        assert!(tokens_from("{}").is_err());
    }

    #[test]
    fn the_id_tokens_nonce_is_read() {
        let payload = base64url(br#"{"sub":"1","nonce":"n-0S6_WzA2Mj"}"#);
        let token = format!("eyJhbGciOiJub25lIn0.{payload}.");
        assert_eq!(nonce_of(&token).as_deref(), Some("n-0S6_WzA2Mj"));
        assert_eq!(nonce_of("garbage"), None);
    }

    #[test]
    fn percent_coding_round_trips() {
        let text = "a b&c=d/é~";
        let encoded = percent_encode(text);
        assert_eq!(encoded, "a%20b%26c%3Dd%2F%C3%A9~");
        assert_eq!(percent_decode(&encoded), text);
        assert_eq!(percent_decode("a+b%2"), "a b%2");
    }
}
