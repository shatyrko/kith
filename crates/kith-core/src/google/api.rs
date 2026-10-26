//! The slice of the Google People API (v1) that the sync needs, as a trait plus a blocking HTTP implementation.
//!
//! * [`PeopleApi`] is what [`crate::google::sync::sync`] talks to; tests use an in-memory fake.
//! * [`HttpPeopleApi`] speaks to `https://people.googleapis.com/v1` with `reqwest` (blocking, rustls).
//!   It retries `429`/`5xx`/network hiccups with exponential backoff (honouring `Retry-After`), sends mutations one
//!   at a time and spaces them out (the write quota is about 60 per minute per user; each call here is one write).
//!   Single-contact calls were chosen over `batchCreate/Update/Delete` on purpose: a failure then affects one contact
//!   and is reported for that contact, the etag of every update is checked individually, and the sync can save its
//!   state after each step. A first push of thousands of contacts is slow (about 1 per 1.1 s) but safe.
//!
//! Not verified against live Google from the development machine: the request shapes follow the public reference.

use crate::google::mapping::PERSON_FIELDS;
use base64::Engine;
use reqwest::blocking::{Client, Response};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    /// The incremental sync token is too old (Google keeps them 7 days): do a full sync.
    #[error("the Google sync token has expired")]
    SyncTokenExpired,
    /// The access token was refused or could not be obtained: sign in again.
    #[error("Google did not accept the sign-in; sign in again")]
    Unauthorized,
    #[error("Google asks to slow down (retry after {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },
    /// The contact changed on Google since it was read (etag mismatch).
    #[error("the contact was changed on Google in the meantime")]
    Conflict,
    #[error("not found on Google")]
    NotFound,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// One page of `people.connections.list`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub people: Vec<Value>,
    pub next_page_token: Option<String>,
    /// Only on the last page.
    pub next_sync_token: Option<String>,
}

/// `(resource_name, name, group_type)`; `group_type` is `USER_CONTACT_GROUP` for labels, `SYSTEM_CONTACT_GROUP` otherwise.
pub type GroupInfo = (String, String, String);

pub const USER_GROUP: &str = "USER_CONTACT_GROUP";

pub trait PeopleApi {
    /// With a sync token only what changed since is returned (deleted people have `metadata.deleted == true`).
    fn list_connections(&self, sync_token: Option<&str>, page_token: Option<&str>) -> Result<Page>;
    fn list_groups(&self) -> Result<Vec<GroupInfo>>;
    fn create_group(&self, name: &str) -> Result<GroupInfo>;
    /// One person, fresh from Google (used before an update to merge and to get the current etag).
    fn get_contact(&self, resource_name: &str) -> Result<Value>;
    fn create_contact(&self, person: &Value) -> Result<Value>;
    /// Replaces the fields named in `update_person_fields` (comma separated). `etag` must be current, else [`ApiError::Conflict`].
    fn update_contact(&self, resource_name: &str, etag: &str, person: &Value, update_person_fields: &str) -> Result<Value>;
    fn delete_contact(&self, resource_name: &str) -> Result<()>;
    fn download_photo(&self, url: &str) -> Result<Vec<u8>>;
    /// Sets the contact's photo; returns the updated person.
    fn set_photo(&self, resource_name: &str, bytes: &[u8]) -> Result<Value>;
}

// ───────────────────────────── error classification ─────────────────────────────

fn error_message(body: &str) -> (String, String) {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = v.get("error").unwrap_or(&Value::Null);
    let message = e.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    let status = e.get("status").and_then(Value::as_str).unwrap_or("").to_string();
    (status, message)
}

/// Maps a failed HTTP response to an [`ApiError`]. Exposed for tests.
pub fn classify(status: u16, retry_after: Option<Duration>, body: &str) -> ApiError {
    let (gstatus, message) = error_message(body);
    let lower = format!("{body} {message}").to_lowercase();
    if status == 410 || body.contains("EXPIRED_SYNC_TOKEN") || (status == 400 && lower.contains("sync token") && lower.contains("expired")) {
        return ApiError::SyncTokenExpired;
    }
    match status {
        401 => ApiError::Unauthorized,
        404 => ApiError::NotFound,
        409 | 412 => ApiError::Conflict,
        429 => ApiError::RateLimited { retry_after },
        400 if gstatus == "FAILED_PRECONDITION" => ApiError::Conflict,
        403 if gstatus == "RESOURCE_EXHAUSTED" || lower.contains("ratelimitexceeded") || lower.contains("quota") => ApiError::RateLimited { retry_after },
        _ => {
            let shown = if message.is_empty() { body.chars().take(300).collect::<String>() } else { message };
            ApiError::Other(format!("Google returned HTTP {status}: {shown}"))
        }
    }
}

// ───────────────────────────── HTTP implementation ─────────────────────────────

type TokenFn = Box<dyn Fn() -> Result<String> + Send + Sync>;

pub struct HttpPeopleApi {
    client: Client,
    token: TokenFn,
    base: String,
    write_gap: Duration,
    backoff_base: Duration,
    max_retries: u32,
    last_write: Mutex<Option<Instant>>,
}

impl HttpPeopleApi {
    /// `token` returns a valid access token each time it is called (see [`crate::google::oauth::access_token`]; it caches).
    pub fn new(token: impl Fn() -> Result<String> + Send + Sync + 'static) -> HttpPeopleApi {
        let client = Client::builder()
            .user_agent(concat!("kith/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new());
        HttpPeopleApi {
            client,
            token: Box::new(token),
            base: "https://people.googleapis.com/v1".into(),
            write_gap: Duration::from_millis(1100),
            backoff_base: Duration::from_secs(4),
            max_retries: 5,
            last_write: Mutex::new(None),
        }
    }

    /// Minimum time between two mutating calls (default 1.1 s, about 55 per minute).
    pub fn with_write_interval(mut self, gap: Duration) -> Self {
        self.write_gap = gap;
        self
    }

    /// Another API root, for tests.
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base = base.trim_end_matches('/').to_string();
        self
    }

    /// First wait after a `429`/`5xx` (doubles each time, capped at 60 s).
    pub fn with_backoff(mut self, base: Duration) -> Self {
        self.backoff_base = base;
        self
    }

    fn throttle(&self) {
        let mut last = match self.last_write.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(t) = *last {
            let since = t.elapsed();
            if since < self.write_gap {
                std::thread::sleep(self.write_gap - since);
            }
        }
        *last = Some(Instant::now());
    }

    fn backoff(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        let exp = self.backoff_base.saturating_mul(1u32 << attempt.min(5));
        retry_after.unwrap_or(exp).min(Duration::from_secs(60))
    }

    /// One logical request with retries. `write` requests are throttled and never run in parallel.
    fn call(&self, method: Method, path: &str, query: &[(&str, String)], body: Option<&Value>, write: bool) -> Result<String> {
        let url = format!("{}/{}", self.base, path.trim_start_matches('/'));
        let mut attempt = 0;
        loop {
            if write {
                self.throttle();
            }
            let token = (self.token)()?;
            let mut req = self.client.request(method.clone(), &url).bearer_auth(token).query(query);
            if let Some(b) = body {
                req = req.json(b);
            }
            let (retry, err) = match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    let retry_after = resp.headers().get("retry-after").and_then(|h| h.to_str().ok()).and_then(|s| s.trim().parse::<u64>().ok()).map(Duration::from_secs);
                    let text = resp.text().map_err(|e| ApiError::Other(format!("reading Google's answer failed: {e}")))?;
                    if status.is_success() {
                        return Ok(text);
                    }
                    let err = classify(status.as_u16(), retry_after, &text);
                    let transient = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() || matches!(err, ApiError::RateLimited { .. });
                    (transient.then_some(retry_after), err)
                }
                Err(e) => (Some(None), ApiError::Other(format!("network error: {e}"))),
            };
            match retry {
                Some(after) if attempt < self.max_retries => {
                    std::thread::sleep(self.backoff(attempt, after));
                    attempt += 1;
                }
                _ => return Err(err),
            }
        }
    }

    fn json(&self, method: Method, path: &str, query: &[(&str, String)], body: Option<&Value>, write: bool) -> Result<Value> {
        let text = self.call(method, path, query, body, write)?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| ApiError::Other(format!("Google sent something unreadable: {e}")))
    }
}

/// `people/c123` only: the name goes into a URL path.
fn check_resource(rn: &str) -> Result<()> {
    let ok = rn.starts_with("people/") && rn.len() > 7 && rn[7..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::Other(format!("unexpected resource name {rn:?}")))
    }
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn group_of(g: &Value) -> GroupInfo {
    let gt = str_of(g, "groupType");
    (str_of(g, "resourceName"), str_of(g, "name"), if gt.is_empty() { USER_GROUP.to_string() } else { gt })
}

const SOURCE: (&str, &str) = ("sources", "READ_SOURCE_TYPE_CONTACT");

/// Photos come from Google's image servers only.
fn photo_host_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && [".googleusercontent.com", ".ggpht.com", ".gstatic.com"].iter().any(|s| host.ends_with(s))
}

impl PeopleApi for HttpPeopleApi {
    fn list_connections(&self, sync_token: Option<&str>, page_token: Option<&str>) -> Result<Page> {
        let mut q = vec![
            ("personFields", PERSON_FIELDS.to_string()),
            ("pageSize", "1000".to_string()),
            ("requestSyncToken", "true".to_string()),
            (SOURCE.0, SOURCE.1.to_string()),
        ];
        if let Some(t) = sync_token {
            q.push(("syncToken", t.to_string()));
        }
        if let Some(t) = page_token {
            q.push(("pageToken", t.to_string()));
        }
        let v = self.json(Method::GET, "people/me/connections", &q, None, false)?;
        Ok(Page {
            people: v.get("connections").and_then(Value::as_array).cloned().unwrap_or_default(),
            next_page_token: Some(str_of(&v, "nextPageToken")).filter(|s| !s.is_empty()),
            next_sync_token: Some(str_of(&v, "nextSyncToken")).filter(|s| !s.is_empty()),
        })
    }

    fn list_groups(&self) -> Result<Vec<GroupInfo>> {
        let mut out = vec![];
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("pageSize", "1000".to_string())];
            if let Some(p) = &page {
                q.push(("pageToken", p.clone()));
            }
            let v = self.json(Method::GET, "contactGroups", &q, None, false)?;
            if let Some(gs) = v.get("contactGroups").and_then(Value::as_array) {
                out.extend(gs.iter().map(group_of));
            }
            page = Some(str_of(&v, "nextPageToken")).filter(|s| !s.is_empty());
            if page.is_none() {
                return Ok(out);
            }
        }
    }

    fn create_group(&self, name: &str) -> Result<GroupInfo> {
        let v = self.json(Method::POST, "contactGroups", &[], Some(&json!({ "contactGroup": { "name": name } })), true)?;
        let g = group_of(&v);
        if g.0.is_empty() {
            return Err(ApiError::Other("Google created the label but did not say how it is called".into()));
        }
        Ok(g)
    }

    fn get_contact(&self, resource_name: &str) -> Result<Value> {
        check_resource(resource_name)?;
        self.json(Method::GET, resource_name, &[("personFields", PERSON_FIELDS.to_string()), (SOURCE.0, SOURCE.1.to_string())], None, false)
    }

    fn create_contact(&self, person: &Value) -> Result<Value> {
        self.json(Method::POST, "people:createContact", &[("personFields", PERSON_FIELDS.to_string()), (SOURCE.0, SOURCE.1.to_string())], Some(person), true)
    }

    fn update_contact(&self, resource_name: &str, etag: &str, person: &Value, update_person_fields: &str) -> Result<Value> {
        check_resource(resource_name)?;
        let mut body = person.clone();
        if let Value::Object(m) = &mut body {
            m.insert("etag".into(), Value::String(etag.to_string()));
            m.insert("resourceName".into(), Value::String(resource_name.to_string()));
        }
        let q = [("updatePersonFields", update_person_fields.to_string()), ("personFields", PERSON_FIELDS.to_string()), (SOURCE.0, SOURCE.1.to_string())];
        self.json(Method::PATCH, &format!("{resource_name}:updateContact"), &q, Some(&body), true)
    }

    fn delete_contact(&self, resource_name: &str) -> Result<()> {
        check_resource(resource_name)?;
        self.call(Method::DELETE, &format!("{resource_name}:deleteContact"), &[], None, true).map(|_| ())
    }

    fn download_photo(&self, url: &str) -> Result<Vec<u8>> {
        if !photo_host_ok(url) {
            return Err(ApiError::Other("refusing to download a photo from an unexpected address".into()));
        }
        // Photo URLs are public: the access token is deliberately not sent.
        let resp: Response = self.client.get(url).send().map_err(|e| ApiError::Other(format!("photo download failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(ApiError::Other(format!("photo download failed: HTTP {}", resp.status().as_u16())));
        }
        const MAX: u64 = 10 * 1024 * 1024;
        let mut buf = vec![];
        resp.take(MAX + 1).read_to_end(&mut buf).map_err(|e| ApiError::Other(format!("photo download failed: {e}")))?;
        if buf.len() as u64 > MAX {
            return Err(ApiError::Other("photo is larger than 10 MB".into()));
        }
        Ok(buf)
    }

    fn set_photo(&self, resource_name: &str, bytes: &[u8]) -> Result<Value> {
        check_resource(resource_name)?;
        let body = json!({ "photoBytes": base64::engine::general_purpose::STANDARD.encode(bytes), "personFields": PERSON_FIELDS, "sources": [SOURCE.1] });
        let v = self.json(Method::POST, &format!("{resource_name}:updateContactPhoto"), &[], Some(&body), true)?;
        Ok(v.get("person").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::testutil::mock;

    #[test]
    fn classification() {
        assert_eq!(classify(410, None, ""), ApiError::SyncTokenExpired);
        let expired = r#"{"error":{"code":400,"message":"Sync token is expired. Clear cache and retry call without the sync token.","status":"FAILED_PRECONDITION","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"EXPIRED_SYNC_TOKEN"}]}}"#;
        assert_eq!(classify(400, None, expired), ApiError::SyncTokenExpired);
        assert_eq!(classify(401, None, "{}"), ApiError::Unauthorized);
        assert_eq!(classify(404, None, "{}"), ApiError::NotFound);
        assert_eq!(classify(409, None, "{}"), ApiError::Conflict);
        assert_eq!(classify(412, None, "{}"), ApiError::Conflict);
        let etag = r#"{"error":{"code":400,"message":"Request person.etag is different than the current person.etag.","status":"FAILED_PRECONDITION"}}"#;
        assert_eq!(classify(400, None, etag), ApiError::Conflict);
        assert_eq!(classify(429, Some(Duration::from_secs(7)), "{}"), ApiError::RateLimited { retry_after: Some(Duration::from_secs(7)) });
        let quota = r#"{"error":{"code":403,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#;
        assert!(matches!(classify(403, None, quota), ApiError::RateLimited { .. }));
        let off = r#"{"error":{"code":403,"message":"People API has not been used in project 1 before or it is disabled.","status":"PERMISSION_DENIED"}}"#;
        match classify(403, None, off) {
            ApiError::Other(m) => assert!(m.contains("has not been used") && m.contains("403")),
            e => panic!("{e:?}"),
        }
        assert!(matches!(classify(500, None, "<html>oops</html>"), ApiError::Other(_)));
    }

    #[test]
    fn resource_names_and_photo_hosts() {
        assert!(check_resource("people/c123456").is_ok());
        for bad in ["people/", "people/../x", "contactGroups/1", "people/c1?x=1", "people/c1/x"] {
            assert!(check_resource(bad).is_err(), "{bad}");
        }
        assert!(photo_host_ok("https://lh3.googleusercontent.com/contacts/abc=s256"));
        assert!(!photo_host_ok("http://lh3.googleusercontent.com/x"));
        assert!(!photo_host_ok("https://evil.example/x?.googleusercontent.com"));
        assert!(!photo_host_ok("https://lh3.googleusercontent.com@evil.example/x"));
        assert!(!photo_host_ok("https://googleusercontent.com.evil.example/x"));
    }

    fn api(base: &str) -> HttpPeopleApi {
        HttpPeopleApi::new(|| Ok("tok".to_string())).with_base_url(base).with_write_interval(Duration::ZERO).with_backoff(Duration::from_millis(5))
    }

    #[test]
    fn list_connections_request_and_page() {
        let body = r#"{"connections":[{"resourceName":"people/c1"}],"nextPageToken":"P2","nextSyncToken":""}"#.to_string();
        let (base, log) = mock(vec![(200, "", body)]);
        let page = api(&base).list_connections(Some("SYNC"), Some("P1")).unwrap();
        assert_eq!(page.people.len(), 1);
        assert_eq!(page.next_page_token.as_deref(), Some("P2"));
        assert_eq!(page.next_sync_token, None);
        let log = log.lock().unwrap();
        let (line, head, _) = &log[0];
        assert!(line.starts_with("GET /v1/people/me/connections?"), "{line}");
        for needle in ["requestSyncToken=true", "pageSize=1000", "syncToken=SYNC", "pageToken=P1", "personFields=names%2C", "sources=READ_SOURCE_TYPE_CONTACT"] {
            assert!(line.contains(needle), "{needle} in {line}");
        }
        assert!(head.to_lowercase().contains("authorization: bearer tok"));
    }

    #[test]
    fn retries_429_and_5xx_then_succeeds() {
        let (base, log) = mock(vec![(429, "Retry-After: 0\r\n", "{}".into()), (503, "", "{}".into()), (200, "", r#"{"nextSyncToken":"T"}"#.into())]);
        let page = api(&base).list_connections(None, None).unwrap();
        assert_eq!(page.next_sync_token.as_deref(), Some("T"));
        assert_eq!(log.lock().unwrap().len(), 3);
    }

    #[test]
    fn gives_up_after_retries_and_reports_error_kinds() {
        let (base, _) = mock(vec![(429, "", "{}".into()); 6]);
        assert!(matches!(api(&base).list_groups(), Err(ApiError::RateLimited { .. })));
        let (base, log) = mock(vec![(410, "", "{}".into())]);
        assert_eq!(api(&base).list_connections(Some("old"), None), Err(ApiError::SyncTokenExpired));
        assert_eq!(log.lock().unwrap().len(), 1, "no retry on 410");
        let (base, _) = mock(vec![(401, "", "{}".into())]);
        assert_eq!(api(&base).list_groups(), Err(ApiError::Unauthorized));
    }

    #[test]
    fn update_sends_etag_mask_and_body() {
        let (base, log) = mock(vec![(200, "", r#"{"resourceName":"people/c1","etag":"E2"}"#.into())]);
        let p = api(&base).update_contact("people/c1", "E1", &json!({"names": [{"givenName": "A"}]}), "names").unwrap();
        assert_eq!(p["etag"], "E2");
        let log = log.lock().unwrap();
        let (line, _, body) = &log[0];
        assert!(line.starts_with("PATCH /v1/people/c1:updateContact?"), "{line}");
        assert!(line.contains("updatePersonFields=names"));
        let b: Value = serde_json::from_str(body).unwrap();
        assert_eq!(b["etag"], "E1");
        assert_eq!(b["names"][0]["givenName"], "A");
    }

    #[test]
    fn groups_create_delete_and_photo() {
        let (base, log) = mock(vec![
            (200, "", r#"{"contactGroups":[{"resourceName":"contactGroups/1","name":"Family","groupType":"USER_CONTACT_GROUP"},{"resourceName":"contactGroups/myContacts","name":"myContacts","groupType":"SYSTEM_CONTACT_GROUP"}]}"#.into()),
            (200, "", r#"{"resourceName":"contactGroups/9","name":"New","groupType":"USER_CONTACT_GROUP"}"#.into()),
            (200, "", "{}".into()),
            (200, "", r#"{"person":{"resourceName":"people/c1","etag":"E3"}}"#.into()),
        ]);
        let a = api(&base);
        let gs = a.list_groups().unwrap();
        assert_eq!(gs[0], ("contactGroups/1".into(), "Family".into(), USER_GROUP.into()));
        assert_eq!(gs[1].2, "SYSTEM_CONTACT_GROUP");
        assert_eq!(a.create_group("New").unwrap().0, "contactGroups/9");
        a.delete_contact("people/c1").unwrap();
        assert_eq!(a.set_photo("people/c1", &[1, 2, 3]).unwrap()["etag"], "E3");
        let log = log.lock().unwrap();
        assert!(log[1].2.contains(r#""name":"New""#));
        assert!(log[2].0.starts_with("DELETE /v1/people/c1:deleteContact"));
        assert!(log[3].0.starts_with("POST /v1/people/c1:updateContactPhoto"));
        assert!(log[3].2.contains("AQID"));
    }

    #[test]
    fn photo_download_refuses_foreign_hosts() {
        let a = api("http://127.0.0.1:1");
        assert!(matches!(a.download_photo("https://example.com/x.jpg"), Err(ApiError::Other(_))));
    }
}
