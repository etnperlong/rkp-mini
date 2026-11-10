//! Remote providers (design §8): GitHub Gist, WebDAV and public HTTPS files.
//!
//! Providers move armored ciphertext only. Credentials (Gist token, WebDAV
//! app password) are read from 0600 secret files and sent only in request
//! headers; they never appear in URLs, errors or logs.

use std::cell::RefCell;

use anyhow::Result;
use base64::Engine;
use rkp_sync_core::error::{err, Stage};
use rkp_sync_core::locator::{self, Location, GIST_RAW_HOST};
use rkp_sync_core::secrecy::{ExposeSecret, SecretString};
use rkp_sync_core::transport::{Fetch, Transport};
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use crate::http::{redact, status_error, Http, Response};

const GITHUB_API: &str = "https://api.github.com";
const GIST_DESCRIPTION: &str = "rkp-sync";
const UPDATED_AT_PREFIX: &str = "updated_at:";

/// Latest-revision raw URL for a gist file.
pub fn gist_raw_url(owner: &str, id: &str, filename: &str) -> String {
    format!("https://{GIST_RAW_HOST}/{owner}/{id}/raw/{filename}")
}

/// A gist created by the first push.
#[derive(Debug, Clone)]
pub struct CreatedGist {
    pub id: String,
    pub url: String,
}

pub struct Gist<'a> {
    http: &'a Http,
    filename: String,
    raw_url: Option<Url>,
    gist_id: RefCell<Option<String>>,
    token: Option<SecretString>,
    created: RefCell<Option<CreatedGist>>,
}

impl<'a> Gist<'a> {
    pub fn new(
        http: &'a Http,
        raw_url: Option<&str>,
        gist_id: Option<&str>,
        filename: &str,
        token: Option<SecretString>,
    ) -> Result<Gist<'a>> {
        let raw_url = raw_url
            .map(|u| {
                Url::parse(u)
                    .map_err(|e| err(Stage::Config, "config_invalid", format!("gist url: {e}")))
            })
            .transpose()?;
        Ok(Gist {
            http,
            filename: filename.to_string(),
            raw_url,
            gist_id: RefCell::new(gist_id.map(str::to_string)),
            token,
            created: RefCell::new(None),
        })
    }

    pub fn created(&self) -> Option<CreatedGist> {
        self.created.borrow().clone()
    }

    fn auth_header(&self) -> Option<Zeroizing<String>> {
        self.token
            .as_ref()
            .map(|t| Zeroizing::new(format!("Bearer {}", t.expose_secret())))
    }

    fn api(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<Response> {
        let url = Url::parse(&format!("{GITHUB_API}{path}")).expect("static base");
        let auth = self.auth_header();
        let mut headers: Vec<(&str, &str)> = vec![
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ];
        if let Some(a) = &auth {
            headers.push(("Authorization", a.as_str()));
        }
        if body.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        self.http.send(method, &url, &headers, body)
    }

    fn api_error(&self, status: u16, what: &str) -> anyhow::Error {
        if status == 403 && self.token.is_none() {
            return err(
                Stage::Remote,
                "rate_limited",
                format!("{what}: HTTP 403 (anonymous GitHub API rate limit?)"),
            );
        }
        status_error(status, what)
    }

    /// Read the file through the REST API (uncached; works without a token).
    fn fetch_api(&self, if_none_match: Option<&str>) -> Result<Fetch> {
        let Some(id) = self.gist_id.borrow().clone() else {
            return Ok(Fetch::Missing);
        };
        let r = self.api("GET", &format!("/gists/{id}"), None)?;
        match r.status {
            200 => {}
            404 => return Ok(Fetch::Missing),
            s => return Err(self.api_error(s, "GET gist")),
        }
        let v: Value = serde_json::from_slice(&r.body).map_err(|_| {
            err(
                Stage::Remote,
                "bad_response",
                "gist API returned invalid JSON",
            )
        })?;
        let token = v["updated_at"]
            .as_str()
            .map(|u| format!("{UPDATED_AT_PREFIX}{u}"));
        if token.is_some() && token.as_deref() == if_none_match {
            return Ok(Fetch::NotModified);
        }
        let file = &v["files"][&self.filename];
        if file.is_null() {
            return Ok(Fetch::Missing);
        }
        let bytes = if file["truncated"].as_bool() == Some(true) {
            let raw = file["raw_url"]
                .as_str()
                .and_then(|u| Url::parse(u).ok())
                .filter(|u| u.scheme() == "https" && u.host_str() == Some(GIST_RAW_HOST))
                .ok_or_else(|| {
                    err(
                        Stage::Remote,
                        "bad_response",
                        "truncated gist file without a gist.githubusercontent.com raw_url",
                    )
                })?;
            let r = self.http.send("GET", &raw, &[], None)?;
            if r.status != 200 {
                return Err(status_error(r.status, "GET gist raw"));
            }
            r.body
        } else {
            file["content"]
                .as_str()
                .ok_or_else(|| err(Stage::Remote, "bad_response", "gist file has no content"))?
                .as_bytes()
                .to_vec()
        };
        Ok(Fetch::Body { bytes, token })
    }

    /// Read via the latest-revision raw URL (no token, CDN may cache).
    fn fetch_raw(&self, url: &Url, if_none_match: Option<&str>) -> Result<Fetch> {
        let mut headers = vec![("Cache-Control", "no-cache")];
        let inm;
        if let Some(t) = if_none_match.filter(|t| !t.starts_with(UPDATED_AT_PREFIX)) {
            inm = t.to_string();
            headers.push(("If-None-Match", inm.as_str()));
        }
        let r = self.http.send("GET", url, &headers, None)?;
        match r.status {
            200 => {
                let token = r.token();
                Ok(Fetch::Body {
                    bytes: r.body,
                    token,
                })
            }
            304 => Ok(Fetch::NotModified),
            404 if self.gist_id.borrow().is_some() => self.fetch_api(if_none_match),
            404 => Ok(Fetch::Missing),
            s => Err(status_error(s, &format!("GET {}", redact(url)))),
        }
    }

    fn files_body(&self, body: &[u8]) -> Result<Vec<u8>> {
        let content = std::str::from_utf8(body)
            .map_err(|_| err(Stage::Generic, "encode", "envelope is not ASCII armor"))?;
        let mut files = serde_json::Map::new();
        files.insert(
            self.filename.clone(),
            serde_json::json!({ "content": content }),
        );
        Ok(serde_json::to_vec(&serde_json::json!({
            "description": GIST_DESCRIPTION,
            "public": false,
            "files": files,
        }))
        .expect("json"))
    }
}

impl Transport for Gist<'_> {
    fn provider(&self) -> &'static str {
        "gist"
    }

    fn location(&self) -> String {
        if let Some(c) = self.created.borrow().as_ref() {
            return c.url.clone();
        }
        match (&self.raw_url, self.gist_id.borrow().as_ref()) {
            (Some(u), _) => u.to_string(),
            (None, Some(id)) => format!("gist:{id}/{}", self.filename),
            (None, None) => format!("gist:(new)/{}", self.filename),
        }
    }

    fn fetch(&self, if_none_match: Option<&str>) -> Result<Fetch> {
        match &self.raw_url {
            Some(u) if self.token.is_none() => self.fetch_raw(u, if_none_match),
            _ => self.fetch_api(if_none_match),
        }
    }

    fn fetch_fresh(&self) -> Result<Fetch> {
        if self.token.is_some() || self.raw_url.is_none() {
            self.fetch_api(None)
        } else {
            self.fetch(None)
        }
    }

    fn put(&self, body: &[u8], expected: Option<&str>) -> Result<()> {
        if self.token.is_none() {
            return Err(err(
                Stage::Config,
                "token_missing",
                "updating a gist needs a token with `gist` scope (gist_token_file)",
            ));
        }
        let payload = self.files_body(body)?;
        let id = self.gist_id.borrow().clone();
        match id {
            None => {
                let r = self.api("POST", "/gists", Some(&payload))?;
                if r.status != 201 {
                    return Err(self.api_error(r.status, "create gist"));
                }
                let v: Value = serde_json::from_slice(&r.body).map_err(|_| {
                    err(
                        Stage::Remote,
                        "bad_response",
                        "gist API returned invalid JSON",
                    )
                })?;
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let owner = v["owner"]["login"].as_str().unwrap_or_default().to_string();
                locator::check_gist_id(&id).map_err(|_| {
                    err(Stage::Remote, "bad_response", "gist API returned a bad id")
                })?;
                let url = gist_raw_url(&owner, &id, &self.filename);
                locator::check_gist_url(&url, &self.filename, Some(&id)).map_err(|_| {
                    err(
                        Stage::Remote,
                        "bad_response",
                        "gist API returned a bad owner",
                    )
                })?;
                *self.gist_id.borrow_mut() = Some(id.clone());
                *self.created.borrow_mut() = Some(CreatedGist { id, url });
                Ok(())
            }
            Some(id) => {
                // No CAS on gists: compare `updated_at` right before writing.
                if let Some(exp) = expected {
                    match self.fetch_api(None)? {
                        Fetch::Body { token, .. } if token.as_deref() != Some(exp) => {
                            return Err(err(
                                Stage::Remote,
                                "remote_conflict",
                                "gist changed since it was read (another writer?)",
                            ))
                        }
                        _ => {}
                    }
                }
                let r = self.api("PATCH", &format!("/gists/{id}"), Some(&payload))?;
                if r.status != 200 {
                    return Err(self.api_error(r.status, "update gist"));
                }
                Ok(())
            }
        }
    }
}

pub struct Webdav<'a> {
    http: &'a Http,
    file_url: Url,
    auth: Option<Zeroizing<String>>,
}

impl<'a> Webdav<'a> {
    pub fn new(
        http: &'a Http,
        file_url: Url,
        user: Option<&str>,
        password: Option<&SecretString>,
    ) -> Webdav<'a> {
        let auth = match (user, password) {
            (Some(u), Some(p)) => {
                let raw = Zeroizing::new(format!("{u}:{}", p.expose_secret()));
                Some(Zeroizing::new(format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
                )))
            }
            _ => None,
        };
        Webdav {
            http,
            file_url,
            auth,
        }
    }

    fn send(
        &self,
        method: &str,
        url: &Url,
        extra: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<Response> {
        let mut headers: Vec<(&str, &str)> = extra.to_vec();
        if let Some(a) = &self.auth {
            headers.push(("Authorization", a.as_str()));
        }
        self.http.send(method, url, &headers, body)
    }

    fn tmp_url(&self) -> Url {
        let mut u = self.file_url.clone();
        let name = u
            .path_segments()
            .and_then(|mut s| s.next_back().map(str::to_string))
            .unwrap_or_default();
        u.path_segments_mut()
            .expect("https url")
            .pop()
            .push(&format!("{name}.tmp"));
        u
    }

    /// MKCOL each missing parent collection of the file.
    fn mkcol_parents(&self) -> Result<()> {
        let segs: Vec<String> = self
            .file_url
            .path_segments()
            .map(|s| s.map(str::to_string).collect())
            .unwrap_or_default();
        let mut u = self.file_url.clone();
        u.set_path("/");
        for seg in &segs[..segs.len().saturating_sub(1)] {
            u.path_segments_mut()
                .expect("https")
                .pop_if_empty()
                .push(seg)
                .push("");
            let r = self.send("MKCOL", &u, &[], None)?;
            match r.status {
                201 | 405 | 301 | 200 => {} // created, or already exists
                s => return Err(status_error(s, "MKCOL")),
            }
        }
        Ok(())
    }

    fn head_token(&self) -> Result<Option<String>> {
        let r = self.send("HEAD", &self.file_url, &[], None)?;
        match r.status {
            200 => Ok(Some(r.token().unwrap_or_default())),
            404 => Ok(None),
            s => Err(status_error(s, "HEAD")),
        }
    }

    fn put_direct(&self, body: &[u8], expected: Option<&str>) -> Result<()> {
        let mut h = vec![("Content-Type", "application/octet-stream")];
        let cond;
        match expected {
            Some(t) if !t.is_empty() => {
                cond = t.to_string();
                h.push(("If-Match", cond.as_str()));
            }
            Some(_) => {}
            None => h.push(("If-None-Match", "*")),
        }
        let r = self.send("PUT", &self.file_url, &h, Some(body))?;
        match r.status {
            200 | 201 | 204 => Ok(()),
            s => Err(status_error(s, "PUT")),
        }
    }
}

impl Transport for Webdav<'_> {
    fn provider(&self) -> &'static str {
        "webdav"
    }

    fn location(&self) -> String {
        self.file_url.to_string()
    }

    fn fetch(&self, if_none_match: Option<&str>) -> Result<Fetch> {
        let mut h = Vec::new();
        if let Some(t) = if_none_match {
            h.push(("If-None-Match", t));
        }
        let r = self.send("GET", &self.file_url, &h, None)?;
        match r.status {
            200 => {
                let token = r.token();
                Ok(Fetch::Body {
                    bytes: r.body,
                    token,
                })
            }
            304 => Ok(Fetch::NotModified),
            404 => Ok(Fetch::Missing),
            s => Err(status_error(s, "GET")),
        }
    }

    /// PUT a temp file, verify the target is unchanged, MOVE over it.
    /// Servers without MOVE fall back to a conditional PUT (If-Match).
    fn put(&self, body: &[u8], expected: Option<&str>) -> Result<()> {
        let tmp = self.tmp_url();
        let ct = [("Content-Type", "application/octet-stream")];
        let mut r = self.send("PUT", &tmp, &ct, Some(body))?;
        if r.status == 409 {
            self.mkcol_parents()?;
            r = self.send("PUT", &tmp, &ct, Some(body))?;
        }
        if !matches!(r.status, 200 | 201 | 204) {
            return Err(status_error(r.status, "PUT temp"));
        }
        let cleanup = || {
            let _ = self.send("DELETE", &tmp, &[], None);
        };
        let current = match self.head_token() {
            Ok(t) => t,
            Err(e) => {
                cleanup();
                return Err(e);
            }
        };
        let changed = match (expected, current.as_deref()) {
            (None, None) => false,
            (Some(exp), Some(cur)) => !exp.is_empty() && !cur.is_empty() && exp != cur,
            _ => true,
        };
        if changed {
            cleanup();
            return Err(err(
                Stage::Remote,
                "remote_conflict",
                "remote file changed since it was read",
            ));
        }
        let dest = self.file_url.to_string();
        let r = self.send(
            "MOVE",
            &tmp,
            &[("Destination", dest.as_str()), ("Overwrite", "T")],
            None,
        )?;
        match r.status {
            201 | 204 | 200 => Ok(()),
            405 | 501 | 502 => {
                cleanup();
                self.put_direct(body, expected)
            }
            s => {
                cleanup();
                Err(status_error(s, "MOVE"))
            }
        }
    }
}

/// Read-only public HTTPS file.
pub struct UrlFile<'a> {
    http: &'a Http,
    url: Url,
}

impl Transport for UrlFile<'_> {
    fn provider(&self) -> &'static str {
        "url"
    }

    fn location(&self) -> String {
        self.url.to_string()
    }

    fn fetch(&self, if_none_match: Option<&str>) -> Result<Fetch> {
        let mut h = vec![("Cache-Control", "no-cache")];
        if let Some(t) = if_none_match {
            h.push(("If-None-Match", t));
        }
        let r = self.http.send("GET", &self.url, &h, None)?;
        match r.status {
            200 => {
                let token = r.token();
                Ok(Fetch::Body {
                    bytes: r.body,
                    token,
                })
            }
            304 => Ok(Fetch::NotModified),
            404 => Ok(Fetch::Missing),
            401 | 403 => Err(err(
                Stage::Remote,
                "auth_required",
                format!(
                    "GET {}: HTTP {}: this file needs authentication; use the webdav provider",
                    redact(&self.url),
                    r.status
                ),
            )),
            s => Err(status_error(s, &format!("GET {}", redact(&self.url)))),
        }
    }

    fn put(&self, _body: &[u8], _expected: Option<&str>) -> Result<()> {
        Err(err(
            Stage::Config,
            "read_only_provider",
            "provider \"url\" is read-only",
        ))
    }
}

/// Provider chosen from configuration.
pub enum Remote<'a> {
    Gist(Gist<'a>),
    Webdav(Webdav<'a>),
    Url(UrlFile<'a>),
}

/// Credentials needed to open a location (loaded by the caller from secret files).
#[derive(Default)]
pub struct Credentials {
    pub gist_token: Option<SecretString>,
    pub webdav_password: Option<SecretString>,
}

impl<'a> Remote<'a> {
    pub fn open(http: &'a Http, loc: &Location, creds: Credentials) -> Result<Remote<'a>> {
        Ok(match loc {
            Location::Gist {
                url,
                gist_id,
                filename,
            } => Remote::Gist(Gist::new(
                http,
                url.as_deref(),
                gist_id.as_deref(),
                filename,
                creds.gist_token,
            )?),
            Location::Webdav { user, .. } => {
                let file_url = loc.webdav_file_url().ok_or_else(|| {
                    err(
                        Stage::Config,
                        "config_invalid",
                        "invalid WebDAV endpoint/path",
                    )
                })?;
                if user.is_some() && creds.webdav_password.is_none() {
                    return Err(err(
                        Stage::Config,
                        "secret_missing",
                        "WebDAV user is set but no password was loaded",
                    ));
                }
                Remote::Webdav(Webdav::new(
                    http,
                    file_url,
                    user.as_deref(),
                    creds.webdav_password.as_ref(),
                ))
            }
            Location::Url { url } => Remote::Url(UrlFile {
                http,
                url: Url::parse(url)
                    .map_err(|e| err(Stage::Config, "config_invalid", format!("url: {e}")))?,
            }),
        })
    }

    pub fn transport(&self) -> &dyn Transport {
        match self {
            Remote::Gist(g) => g,
            Remote::Webdav(w) => w,
            Remote::Url(u) => u,
        }
    }

    pub fn created_gist(&self) -> Option<CreatedGist> {
        match self {
            Remote::Gist(g) => g.created(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_sync_core::locator::UrlPolicy;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    type Files = Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>>;

    /// Minimal in-memory WebDAV server: GET/HEAD/PUT/MOVE/DELETE/MKCOL with ETags.
    fn webdav_server(support_move: bool) -> (String, Files, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let files = Arc::new(Mutex::new(
            std::collections::HashMap::<String, Vec<u8>>::new(),
        ));
        let log = Arc::new(Mutex::new(Vec::new()));
        let (f2, l2, b2) = (files.clone(), log.clone(), base.clone());
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).unwrap() == 0 {
                    continue;
                }
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap().to_string();
                let path = parts.next().unwrap().to_string();
                let mut headers = std::collections::HashMap::new();
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let (k, v) = line.split_once(':').unwrap();
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
                let len: usize = headers
                    .get("content-length")
                    .map_or(0, |v| v.parse().unwrap());
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                l2.lock().unwrap().push(format!("{method} {path}"));
                let mut files = f2.lock().unwrap();
                let etag = |b: &[u8]| format!("\"{}\"", &rkp_sync_core::sha256_hex(b)[..8]);
                let (status, extra, out): (u16, String, Vec<u8>) = match method.as_str() {
                    "GET" | "HEAD" => match files.get(&path) {
                        Some(b) if headers.get("if-none-match") == Some(&etag(b)) => {
                            (304, String::new(), vec![])
                        }
                        Some(b) => (
                            200,
                            format!("ETag: {}\r\n", etag(b)),
                            if method == "GET" { b.clone() } else { vec![] },
                        ),
                        None => (404, String::new(), vec![]),
                    },
                    "PUT" => {
                        let cur = files.get(&path).map(|b| etag(b));
                        let ok = match (headers.get("if-match"), headers.get("if-none-match")) {
                            (Some(m), _) => cur.as_ref() == Some(m),
                            (_, Some(_)) => cur.is_none(),
                            _ => true,
                        };
                        if ok {
                            files.insert(path.clone(), body);
                            (201, String::new(), vec![])
                        } else {
                            (412, String::new(), vec![])
                        }
                    }
                    "MOVE" if support_move => {
                        let dest = headers["destination"]
                            .strip_prefix(&b2)
                            .unwrap()
                            .to_string();
                        let b = files.remove(&path).unwrap();
                        files.insert(dest, b);
                        (204, String::new(), vec![])
                    }
                    "MOVE" => (405, String::new(), vec![]),
                    "DELETE" => {
                        files.remove(&path);
                        (204, String::new(), vec![])
                    }
                    "MKCOL" => (201, String::new(), vec![]),
                    _ => (400, String::new(), vec![]),
                };
                let head_len = if method == "HEAD" {
                    files.get(&path).map_or(0, Vec::len)
                } else {
                    out.len()
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\n{extra}Content-Length: {head_len}\r\nConnection: close\r\n\r\n"
                );
                s.write_all(resp.as_bytes()).unwrap();
                s.write_all(&out).unwrap();
            }
        });
        (base, files, log)
    }

    fn dev_http() -> Http {
        Http::new(
            5,
            "",
            UrlPolicy {
                allow_insecure_http: true,
                allow_private: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn webdav_put_move_and_conditional_get() {
        let (base, files, log) = webdav_server(true);
        let http = dev_http();
        let pw = SecretString::from("app-pass".to_string());
        let url = Url::parse(&format!("{base}/rkp/x.age")).unwrap();
        let w = Webdav::new(&http, url, Some("me"), Some(&pw));
        assert_eq!(w.fetch(None).unwrap(), Fetch::Missing);
        w.put(b"v1", None).unwrap();
        assert_eq!(files.lock().unwrap()["/rkp/x.age"], b"v1");
        assert!(!files.lock().unwrap().contains_key("/rkp/x.age.tmp"));
        let Fetch::Body { token, .. } = w.fetch(None).unwrap() else {
            panic!()
        };
        assert_eq!(w.fetch(token.as_deref()).unwrap(), Fetch::NotModified);
        // Stale expectation → conflict, target untouched, temp cleaned.
        let e = w.put(b"v2", Some("\"stale\"")).unwrap_err();
        assert_eq!(
            rkp_sync_core::error::to_sync_error(&e).code,
            "remote_conflict"
        );
        assert_eq!(files.lock().unwrap()["/rkp/x.age"], b"v1");
        assert!(!files.lock().unwrap().contains_key("/rkp/x.age.tmp"));
        w.put(b"v2", token.as_deref()).unwrap();
        assert_eq!(files.lock().unwrap()["/rkp/x.age"], b"v2");
        assert!(log
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("MOVE /rkp/x.age.tmp")));
    }

    #[test]
    fn webdav_falls_back_to_if_match_without_move() {
        let (base, files, _) = webdav_server(false);
        let http = dev_http();
        let url = Url::parse(&format!("{base}/x.age")).unwrap();
        let w = Webdav::new(&http, url, None, None);
        w.put(b"v1", None).unwrap();
        let Fetch::Body { token, .. } = w.fetch(None).unwrap() else {
            panic!()
        };
        w.put(b"v2", token.as_deref()).unwrap();
        assert_eq!(files.lock().unwrap()["/x.age"], b"v2");
        assert!(!files.lock().unwrap().contains_key("/x.age.tmp"));
    }

    #[test]
    fn url_provider_is_read_only_and_maps_auth() {
        let (base, files, _) = webdav_server(true);
        files
            .lock()
            .unwrap()
            .insert("/pub.age".into(), b"cipher".to_vec());
        let http = dev_http();
        let u = UrlFile {
            http: &http,
            url: Url::parse(&format!("{base}/pub.age")).unwrap(),
        };
        let Fetch::Body { bytes, .. } = u.fetch(None).unwrap() else {
            panic!()
        };
        assert_eq!(bytes, b"cipher");
        assert!(u.put(b"x", None).is_err());
    }
}
