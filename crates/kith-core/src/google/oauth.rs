//! Google sign-in for a desktop app: OAuth 2.0 authorization-code flow with a loopback redirect and PKCE (S256).
//!
//! Flow, for a Tauri app that opens the browser itself:
//!
//! 1. [`start`] binds `127.0.0.1` on a random port and returns a [`LoginHandle`] whose `auth_url` you open in the browser.
//! 2. [`LoginHandle::wait`] (blocking: run it on a worker thread) serves the one redirect request, checks `state`,
//!    shows a small "you can close this tab" page, exchanges the code at Google's token endpoint, reads the account
//!    e-mail from the `id_token` (or the userinfo endpoint), and stores the refresh token in the [`TokenStore`].
//! 3. From then on [`access_token`] returns a valid access token, refreshing and caching it as needed.
//!
//! The client id and secret are the user's own (see `docs/google-setup.md`). The refresh token lives in the Secret
//! Service (GNOME Keyring / KWallet) through the `keyring` crate, with a `0600` file as fallback.

use crate::model::now;
use crate::settings::GoogleSettings;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::blocking::Client;
use reqwest::Url;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SCOPE_CONTACTS: &str = "https://www.googleapis.com/auth/contacts";
/// What Kith asks for: read/write contacts, and the account's e-mail address to show which account is connected.
pub const SCOPES: &str = "https://www.googleapis.com/auth/contacts openid email";

/// Google's endpoints; replaceable for tests.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub userinfo: String,
    pub revoke: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Google sign-in is not set up: enter the client id and secret in the Google settings")]
    NotConfigured,
    #[error("not signed in to Google")]
    NotSignedIn,
    /// The refresh token was revoked or expired (a project in "Testing" mode loses it after 7 days).
    #[error("Google no longer accepts the saved sign-in ({0}); sign in again")]
    Unauthorized(String),
    #[error("sign-in timed out")]
    Timeout,
    #[error("sign-in was refused: {0}")]
    Denied(String),
    #[error("the browser answer did not match this sign-in attempt (wrong state)")]
    StateMismatch,
    #[error("Google did not grant access to contacts; tick the contacts permission when signing in")]
    MissingScope,
    #[error("network problem: {0}")]
    Network(String),
    #[error("unexpected answer from Google: {0}")]
    Protocol(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for crate::google::api::ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::NotConfigured | Error::NotSignedIn | Error::Unauthorized(_) => crate::google::api::ApiError::Unauthorized,
            other => crate::google::api::ApiError::Other(other.to_string()),
        }
    }
}

/// A set of tokens. `refresh_token` is only present right after sign-in.
#[derive(Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: u64,
    /// The signed-in account (empty after a plain refresh).
    pub email: String,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens").field("expires_at", &self.expires_at).field("email", &self.email).field("secrets", &"<hidden>").finish()
    }
}

// ───────────────────────────── PKCE and URLs ─────────────────────────────

/// `n` random bytes, base64url without padding (from the OS random source through UUID v4s).
fn random_token(bytes: usize) -> String {
    let mut raw = Vec::with_capacity(bytes + 16);
    while raw.len() < bytes {
        raw.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    raw.truncate(bytes);
    URL_SAFE_NO_PAD.encode(raw)
}

/// RFC 7636 `code_challenge` for the S256 method.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The page the user opens to grant access.
pub fn build_auth_url(endpoints: &Endpoints, client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> Result<String> {
    let params = [
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        // a refresh token is only handed out with offline access, and again only when consent is shown
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    Url::parse_with_params(&endpoints.auth, params).map(|u| u.to_string()).map_err(|e| Error::Protocol(format!("bad authorization URL: {e}")))
}

/// What the browser's redirect request said.
#[derive(Debug, PartialEq, Eq)]
pub enum Redirect {
    Code(String),
    Denied(String),
    /// Not the redirect (favicon, a probe…): keep waiting.
    Ignore,
}

/// Parses the first line of the HTTP request that reached the loopback listener, e.g. `GET /?code=…&state=… HTTP/1.1`.
pub fn parse_redirect(request_line: &str, expected_state: &str) -> Result<Redirect> {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else { return Ok(Redirect::Ignore) };
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else { return Ok(Redirect::Ignore) };
    if url.path() != "/" {
        return Ok(Redirect::Ignore);
    }
    let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    let (code, error, state) = (get("code"), get("error"), get("state"));
    if code.is_none() && error.is_none() {
        return Ok(Redirect::Ignore);
    }
    if state.as_deref() != Some(expected_state) {
        return Err(Error::StateMismatch);
    }
    match (error, code) {
        (Some(e), _) => Ok(Redirect::Denied(e)),
        (None, Some(c)) if !c.is_empty() => Ok(Redirect::Code(c)),
        _ => Ok(Redirect::Ignore),
    }
}

// ───────────────────────────── sign-in ─────────────────────────────

pub struct LoginHandle {
    /// Open this in the user's browser.
    pub auth_url: String,
    listener: TcpListener,
    state: String,
    verifier: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    endpoints: Endpoints,
    store: TokenStore,
}

/// Starts a sign-in: listens on a random loopback port and builds the authorization URL.
pub fn start(client_id: &str, client_secret: &str) -> Result<LoginHandle> {
    start_with(client_id, client_secret, Endpoints::default(), TokenStore::new())
}

/// [`start`] with other endpoints and token store (tests).
pub fn start_with(client_id: &str, client_secret: &str, endpoints: Endpoints, store: TokenStore) -> Result<LoginHandle> {
    if client_id.trim().is_empty() {
        return Err(Error::NotConfigured);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = random_token(48); // 64 characters, within RFC 7636's 43..128
    let state = random_token(24);
    let auth_url = build_auth_url(&endpoints, client_id.trim(), &redirect_uri, &state, &pkce_challenge(&verifier))?;
    Ok(LoginHandle { auth_url, listener, state, verifier, redirect_uri, client_id: client_id.trim().into(), client_secret: client_secret.trim().into(), endpoints, store })
}

fn page(title: &str, message: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Kith</title><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <style>body{{font-family:system-ui,sans-serif;background:#f4f1ea;color:#2a2a2a;display:grid;place-items:center;height:100vh;margin:0}}\
         main{{max-width:28rem;padding:2rem;text-align:center}}h1{{font-size:1.4rem}}@media(prefers-color-scheme:dark){{body{{background:#1f1f1f;color:#e6e6e6}}}}</style></head>\
         <body><main><h1>{}</h1><p>{}</p></main></body></html>",
        esc(title),
        esc(message)
    )
}

fn respond(stream: &mut TcpStream, status: &str, html: &str) {
    let resp = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{html}", html.len());
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// First line of the request, or `None` if the connection sent nothing usable (browsers open speculative connections).
fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 && !buf.windows(2).any(|w| w == b"\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    text.lines().next().map(str::to_string).filter(|l| !l.is_empty())
}

fn http() -> Client {
    Client::builder().user_agent(concat!("kith/", env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(30)).build().unwrap_or_else(|_| Client::new())
}

impl LoginHandle {
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Waits for the browser to come back (up to `timeout`), then completes the sign-in and stores the refresh token.
    /// Blocking: call it from a worker thread.
    pub fn wait(self, timeout: Duration) -> Result<Tokens> {
        let deadline = Instant::now() + timeout;
        let code = loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            let mut stream = match self.listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let Some(line) = read_request_line(&mut stream) else { continue };
            match parse_redirect(&line, &self.state) {
                Ok(Redirect::Code(c)) => {
                    respond(&mut stream, "200 OK", &page("Kith is connected to Google", "You can close this tab and go back to Kith."));
                    break c;
                }
                Ok(Redirect::Denied(e)) => {
                    respond(&mut stream, "200 OK", &page("Sign-in was cancelled", "Google did not give Kith access. You can close this tab and try again in Kith."));
                    return Err(Error::Denied(e));
                }
                Ok(Redirect::Ignore) => respond(&mut stream, "404 Not Found", &page("Nothing here", "Kith is waiting for the Google sign-in to finish.")),
                // somebody else knocked on the port: refuse and keep waiting for the real answer
                Err(_) => respond(&mut stream, "400 Bad Request", &page("This sign-in link is not valid", "Start the sign-in again from Kith.")),
            }
        };
        let tokens = self.exchange(&code)?;
        if let Some(rt) = &tokens.refresh_token {
            self.store.save(rt)?;
        }
        Ok(tokens)
    }

    fn exchange(&self, code: &str) -> Result<Tokens> {
        let mut form = vec![
            ("code", code),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", self.verifier.as_str()),
        ];
        if !self.client_secret.is_empty() {
            form.push(("client_secret", &self.client_secret));
        }
        let client = http();
        let v = post_token(&client, &self.endpoints.token, &form)?;
        let scope = v.get("scope").and_then(Value::as_str).unwrap_or("");
        if !scope.split_whitespace().any(|s| s == SCOPE_CONTACTS) {
            return Err(Error::MissingScope);
        }
        let mut t = tokens_from(&v)?;
        if t.refresh_token.is_none() {
            return Err(Error::Protocol("Google did not return a refresh token; remove Kith under myaccount.google.com/permissions and sign in again".into()));
        }
        t.email = v.get("id_token").and_then(Value::as_str).and_then(email_from_id_token).unwrap_or_default();
        if t.email.is_empty() {
            t.email = fetch_email(&client, &self.endpoints.userinfo, &t.access_token).unwrap_or_default();
        }
        Ok(t)
    }
}

fn post_token(client: &Client, url: &str, form: &[(&str, &str)]) -> Result<Value> {
    let resp = client.post(url).form(form).send().map_err(|e| Error::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| Error::Network(e.to_string()))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(v);
    }
    let code = v.get("error").and_then(Value::as_str).unwrap_or("");
    let desc = v.get("error_description").and_then(Value::as_str).unwrap_or("");
    let msg = format!("{code} {desc}").trim().to_string();
    if code == "invalid_grant" || code == "invalid_client" || code == "unauthorized_client" {
        Err(Error::Unauthorized(msg))
    } else {
        Err(Error::Protocol(format!("token endpoint answered HTTP {}: {}", status.as_u16(), if msg.is_empty() { text.chars().take(200).collect() } else { msg })))
    }
}

fn tokens_from(v: &Value) -> Result<Tokens> {
    let access = v.get("access_token").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Error::Protocol("no access token in the answer".into()))?;
    let ttl = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
    Ok(Tokens {
        access_token: access.to_string(),
        refresh_token: v.get("refresh_token").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
        expires_at: now() + ttl,
        email: String::new(),
    })
}

/// The `email` claim of an id_token (received straight from Google over TLS, so the signature is not re-checked).
pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("email").and_then(Value::as_str).map(str::to_string)
}

fn fetch_email(client: &Client, url: &str, access_token: &str) -> Option<String> {
    let v: Value = client.get(url).bearer_auth(access_token).send().ok()?.json().ok()?;
    v.get("email").and_then(Value::as_str).map(str::to_string)
}

// ───────────────────────────── refresh and cache ─────────────────────────────

/// Gets a new access token with the refresh token.
pub fn refresh(client_id: &str, client_secret: &str, refresh_token: &str) -> Result<Tokens> {
    refresh_with(&Endpoints::default(), client_id, client_secret, refresh_token)
}

pub fn refresh_with(endpoints: &Endpoints, client_id: &str, client_secret: &str, refresh_token: &str) -> Result<Tokens> {
    let mut form = vec![("client_id", client_id), ("refresh_token", refresh_token), ("grant_type", "refresh_token")];
    if !client_secret.is_empty() {
        form.push(("client_secret", client_secret));
    }
    let v = post_token(&http(), &endpoints.token, &form)?;
    let mut t = tokens_from(&v)?;
    t.refresh_token = Some(refresh_token.to_string());
    t.email = v.get("id_token").and_then(Value::as_str).and_then(email_from_id_token).unwrap_or_default();
    Ok(t)
}

struct Cached {
    client_id: String,
    token: String,
    expires_at: u64,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

fn cache() -> std::sync::MutexGuard<'static, Option<Cached>> {
    CACHE.lock().unwrap_or_else(|p| p.into_inner())
}

/// A valid access token for the signed-in account: the cached one while it lasts, else a refreshed one.
pub fn access_token(settings: &GoogleSettings) -> Result<String> {
    access_token_with(&TokenStore::new(), &Endpoints::default(), settings)
}

pub fn access_token_with(store: &TokenStore, endpoints: &Endpoints, settings: &GoogleSettings) -> Result<String> {
    let client_id = settings.client_id.trim();
    if client_id.is_empty() {
        return Err(Error::NotConfigured);
    }
    {
        let c = cache();
        if let Some(c) = c.as_ref() {
            if c.client_id == client_id && c.expires_at > now() + 60 {
                return Ok(c.token.clone());
            }
        }
    }
    let refresh_token = store.load()?.ok_or(Error::NotSignedIn)?;
    let t = match refresh_with(endpoints, client_id, settings.client_secret.trim(), &refresh_token) {
        Ok(t) => t,
        Err(e) => {
            *cache() = None;
            return Err(e);
        }
    };
    *cache() = Some(Cached { client_id: client_id.to_string(), token: t.access_token.clone(), expires_at: t.expires_at });
    Ok(t.access_token)
}

/// Forgets the account: tells Google to revoke the refresh token (best effort), deletes it locally, drops the cache.
pub fn sign_out(settings: &GoogleSettings) -> Result<()> {
    sign_out_with(&TokenStore::new(), &Endpoints::default(), settings)
}

pub fn sign_out_with(store: &TokenStore, endpoints: &Endpoints, _settings: &GoogleSettings) -> Result<()> {
    *cache() = None;
    if let Ok(Some(rt)) = store.load() {
        let _ = http().post(&endpoints.revoke).form(&[("token", rt.as_str())]).send();
    }
    store.clear()
}

// ───────────────────────────── token storage ─────────────────────────────

const KEYRING_SERVICE: &str = "kith";
const KEYRING_USER: &str = "google-refresh-token";

/// Where the refresh token is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoredIn {
    SecretService,
    File(PathBuf),
}

/// Keeps the refresh token in the Secret Service (via `keyring`), falling back to `~/.config/kith/google-token.json` (0600).
#[derive(Clone, Debug)]
pub struct TokenStore {
    file: PathBuf,
    use_keyring: bool,
}

impl Default for TokenStore {
    fn default() -> Self {
        TokenStore::new()
    }
}

impl TokenStore {
    pub fn new() -> TokenStore {
        let file = crate::settings::Settings::path().with_file_name("google-token.json");
        TokenStore { file, use_keyring: true }
    }

    /// Only the file at `path`, never the keyring (tests, headless machines).
    pub fn file_only(path: impl Into<PathBuf>) -> TokenStore {
        TokenStore { file: path.into(), use_keyring: false }
    }

    pub fn file_path(&self) -> &std::path::Path {
        &self.file
    }

    fn entry(&self) -> Option<keyring::Entry> {
        if !self.use_keyring {
            return None;
        }
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()
    }

    pub fn save(&self, refresh_token: &str) -> Result<StoredIn> {
        if let Some(e) = self.entry() {
            if e.set_password(refresh_token).is_ok() && e.get_password().map(|p| p == refresh_token).unwrap_or(false) {
                let _ = std::fs::remove_file(&self.file);
                return Ok(StoredIn::SecretService);
            }
        }
        self.write_file(refresh_token)?;
        Ok(StoredIn::File(self.file.clone()))
    }

    pub fn load(&self) -> Result<Option<String>> {
        if let Some(e) = self.entry() {
            if let Ok(p) = e.get_password() {
                if !p.is_empty() {
                    return Ok(Some(p));
                }
            }
        }
        match std::fs::read_to_string(&self.file) {
            Ok(t) => Ok(serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("refresh_token").and_then(Value::as_str).map(str::to_string)).filter(|s| !s.is_empty())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn clear(&self) -> Result<()> {
        if let Some(e) = self.entry() {
            let _ = e.delete_credential();
        }
        match std::fs::remove_file(&self.file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_file(&self, refresh_token: &str) -> Result<()> {
        if let Some(d) = self.file.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600); // never readable by others, not even for a moment
            }
            let mut f = opts.open(&tmp)?;
            f.write_all(serde_json::json!({ "refresh_token": refresh_token, "saved_at": now() }).to_string().as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::testutil::mock;
    use std::collections::HashMap;

    fn query_of(url: &str) -> HashMap<String, String> {
        Url::parse(url).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }

    #[test]
    fn pkce_matches_rfc7636_example() {
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn random_tokens_are_long_and_different() {
        let (a, b) = (random_token(48), random_token(48));
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn auth_url_has_everything_google_needs() {
        let url = build_auth_url(&Endpoints::default(), "id.apps.googleusercontent.com", "http://127.0.0.1:5555", "st ate", "chal").unwrap();
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        let q = query_of(&url);
        assert_eq!(q["client_id"], "id.apps.googleusercontent.com");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["scope"], "https://www.googleapis.com/auth/contacts openid email");
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "st ate");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert!(!url.contains(' '));
    }

    #[test]
    fn redirect_parsing() {
        assert_eq!(parse_redirect("GET /?code=4%2F0AbC&state=S&scope=x HTTP/1.1", "S").unwrap(), Redirect::Code("4/0AbC".into()));
        assert_eq!(parse_redirect("GET /?error=access_denied&state=S HTTP/1.1", "S").unwrap(), Redirect::Denied("access_denied".into()));
        assert!(matches!(parse_redirect("GET /?code=abc&state=OTHER HTTP/1.1", "S"), Err(Error::StateMismatch)));
        assert!(matches!(parse_redirect("GET /?code=abc HTTP/1.1", "S"), Err(Error::StateMismatch)));
        assert!(matches!(parse_redirect("GET /?error=access_denied&state=X HTTP/1.1", "S"), Err(Error::StateMismatch)));
        for line in ["GET /favicon.ico HTTP/1.1", "GET / HTTP/1.1", "POST /?code=a&state=S HTTP/1.1", "", "garbage", "GET /other?code=a&state=S HTTP/1.1"] {
            assert_eq!(parse_redirect(line, "S").unwrap(), Redirect::Ignore, "{line}");
        }
    }

    #[test]
    fn id_token_email() {
        let payload = URL_SAFE_NO_PAD.encode(r#"{"iss":"https://accounts.google.com","email":"me@example.com","email_verified":true}"#);
        assert_eq!(email_from_id_token(&format!("xx.{payload}.sig")).as_deref(), Some("me@example.com"));
        assert_eq!(email_from_id_token("nonsense"), None);
        assert_eq!(email_from_id_token("a.!!!.c"), None);
    }

    #[test]
    fn token_debug_hides_secrets() {
        let t = Tokens { access_token: "SECRET-A".into(), refresh_token: Some("SECRET-R".into()), expires_at: 1, email: "e@x".into() };
        let s = format!("{t:?}");
        assert!(!s.contains("SECRET") && s.contains("e@x"));
    }

    #[test]
    fn file_token_store_round_trip_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::file_only(dir.path().join("sub/google-token.json"));
        assert_eq!(store.load().unwrap(), None);
        assert_eq!(store.save("r-token").unwrap(), StoredIn::File(store.file_path().to_path_buf()));
        assert_eq!(store.load().unwrap().as_deref(), Some("r-token"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(store.file_path()).unwrap().permissions().mode() & 0o777, 0o600);
        }
        store.save("r2").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("r2"));
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap();
    }

    /// Plays the browser: opens the redirect URL on the loopback port.
    fn browser_visits(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn full_login_flow_against_a_fake_google() {
        let id_token = format!("h.{}.s", URL_SAFE_NO_PAD.encode(r#"{"email":"me@example.com"}"#));
        let token_body = serde_json::json!({"access_token": "AT", "expires_in": 3599, "refresh_token": "RT", "scope": "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/contacts", "id_token": id_token}).to_string();
        let (base, log) = mock(vec![(200, "", token_body)]);
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::file_only(dir.path().join("t.json"));
        let endpoints = Endpoints { token: format!("{base}/token"), ..Endpoints::default() };
        let login = start_with("cid", "csecret", endpoints, store.clone()).unwrap();
        let q = query_of(&login.auth_url);
        let (port, state) = (login.port(), q["state"].clone());
        assert_eq!(q["redirect_uri"], format!("http://127.0.0.1:{port}"));
        let challenge = q["code_challenge"].clone();
        let verifier = login.verifier.clone();
        assert_eq!(pkce_challenge(&verifier), challenge);

        let waiter = std::thread::spawn(move || login.wait(Duration::from_secs(20)));
        // a wrong-state visit and a favicon request do not end the wait
        assert!(browser_visits(port, "/?code=evil&state=nope").contains("400"));
        assert!(browser_visits(port, "/favicon.ico").contains("404"));
        let page = browser_visits(port, &format!("/?code=THE-CODE&state={state}"));
        assert!(page.contains("200 OK") && page.contains("connected"));
        let tokens = waiter.join().unwrap().unwrap();
        assert_eq!(tokens.access_token, "AT");
        assert_eq!(tokens.refresh_token.as_deref(), Some("RT"));
        assert_eq!(tokens.email, "me@example.com");
        assert!(tokens.expires_at > now() + 3000);
        assert_eq!(store.load().unwrap().as_deref(), Some("RT"));

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        let body = &log[0].2;
        for needle in ["code=THE-CODE", "client_id=cid", "client_secret=csecret", "grant_type=authorization_code", &format!("code_verifier={verifier}")] {
            assert!(body.contains(needle), "{needle} in {body}");
        }
        assert!(body.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"));
    }

    #[test]
    fn login_denied_times_out_and_missing_scope() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::file_only(dir.path().join("t.json"));
        let login = start_with("cid", "s", Endpoints::default(), store.clone()).unwrap();
        let (port, state) = (login.port(), query_of(&login.auth_url)["state"].clone());
        let waiter = std::thread::spawn(move || login.wait(Duration::from_secs(20)));
        browser_visits(port, &format!("/?error=access_denied&state={state}"));
        assert!(matches!(waiter.join().unwrap(), Err(Error::Denied(e)) if e == "access_denied"));

        let login = start_with("cid", "s", Endpoints::default(), store.clone()).unwrap();
        assert!(matches!(login.wait(Duration::from_millis(150)), Err(Error::Timeout)));

        let (base, _) = mock(vec![(200, "", r#"{"access_token":"AT","refresh_token":"RT","scope":"openid email"}"#.into())]);
        let login = start_with("cid", "s", Endpoints { token: format!("{base}/token"), ..Endpoints::default() }, store.clone()).unwrap();
        let (port, state) = (login.port(), query_of(&login.auth_url)["state"].clone());
        let waiter = std::thread::spawn(move || login.wait(Duration::from_secs(20)));
        browser_visits(port, &format!("/?code=c&state={state}"));
        assert!(matches!(waiter.join().unwrap(), Err(Error::MissingScope)));
        assert_eq!(store.load().unwrap(), None, "nothing is stored after a failed sign-in");

        assert!(matches!(start("  ", "x"), Err(Error::NotConfigured)));
    }

    #[test]
    fn access_token_refreshes_caches_and_reports_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::file_only(dir.path().join("t.json"));
        let settings = GoogleSettings { client_id: "cid-cache-test".into(), client_secret: "s".into(), ..Default::default() };
        let (base, log) = mock(vec![(200, "", r#"{"access_token":"AT1","expires_in":3600}"#.into()), (400, "", r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#.into())]);
        let endpoints = Endpoints { token: format!("{base}/token"), ..Endpoints::default() };
        assert!(matches!(access_token_with(&store, &endpoints, &settings), Err(Error::NotSignedIn)));
        store.save("RT").unwrap();
        assert_eq!(access_token_with(&store, &endpoints, &settings).unwrap(), "AT1");
        assert_eq!(access_token_with(&store, &endpoints, &settings).unwrap(), "AT1", "second call comes from the cache");
        assert_eq!(log.lock().unwrap().len(), 1);
        assert!(log.lock().unwrap()[0].2.contains("grant_type=refresh_token"));
        // force a refresh by dropping the cache
        *cache() = None;
        assert!(matches!(access_token_with(&store, &endpoints, &settings), Err(Error::Unauthorized(_))));
        assert!(matches!(access_token_with(&store, &endpoints, &GoogleSettings::default()), Err(Error::NotConfigured)));
        let e: crate::google::api::ApiError = Error::Unauthorized("x".into()).into();
        assert_eq!(e, crate::google::api::ApiError::Unauthorized);
    }
}
