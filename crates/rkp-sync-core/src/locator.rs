//! `rkpsync://v1/import?p=<base64url(JSON)>` parsing and URL policy (design §9).
//!
//! The import payload names a *location* only. It is untrusted: secret-looking
//! and unknown fields are rejected, URLs must be HTTPS without userinfo and
//! (by default) must not point at loopback/private/link-local hosts, WebDAV
//! paths are normalised and may not escape with `..`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::Result;
use base64::Engine;
use serde_json::{Map, Value};
use url::{Host, Url};

use crate::error::{err, Stage};
use crate::{limits, valid_stream_id, REMOTE_FILENAME};

pub const SCHEME_PREFIX: &str = "rkpsync://v1/import?p=";
pub const FORMAT: &str = "age-armor";
pub const GIST_RAW_HOST: &str = "gist.githubusercontent.com";
pub const MAX_LABEL_CHARS: usize = 64;

/// Network policy for remote URLs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UrlPolicy {
    /// Accept `http://` (local debugging only).
    pub allow_insecure_http: bool,
    /// Accept loopback/private/link-local hosts (local debugging only).
    pub allow_private: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Gist {
        /// Latest-revision raw URL (preferred).
        url: Option<String>,
        gist_id: Option<String>,
        filename: String,
    },
    Webdav {
        /// Collection URL, normalised to end with `/`.
        endpoint: String,
        /// Absolute, normalised file path relative to `endpoint`.
        path: String,
        user: Option<String>,
    },
    Url {
        url: String,
    },
}

impl Location {
    pub fn provider(&self) -> &'static str {
        match self {
            Location::Gist { .. } => "gist",
            Location::Webdav { .. } => "webdav",
            Location::Url { .. } => "url",
        }
    }

    /// Public, credential-free description used in state files and output.
    pub fn display(&self) -> String {
        match self {
            Location::Gist { url: Some(u), .. } => u.clone(),
            Location::Gist {
                url: None,
                gist_id,
                filename,
            } => format!("gist:{}/{}", gist_id.as_deref().unwrap_or("?"), filename),
            Location::Webdav { .. } => self.webdav_file_url().expect("webdav").to_string(),
            Location::Url { url } => url.clone(),
        }
    }

    /// Full file URL for a WebDAV location (endpoint joined with path).
    pub fn webdav_file_url(&self) -> Option<Url> {
        let Location::Webdav { endpoint, path, .. } = self else {
            return None;
        };
        let mut u = Url::parse(endpoint).ok()?;
        {
            let mut segs = u.path_segments_mut().ok()?;
            segs.pop_if_empty();
            for s in path.split('/').filter(|s| !s.is_empty()) {
                segs.push(s);
            }
        }
        Some(u)
    }

    /// Validate every field under `policy`; returns the normalised location.
    pub fn validated(self, policy: UrlPolicy) -> Result<Location> {
        match self {
            Location::Gist {
                url,
                gist_id,
                filename,
            } => {
                check_filename(&filename)?;
                if let Some(id) = &gist_id {
                    check_gist_id(id)?;
                }
                let url = match url {
                    Some(u) => Some(check_gist_url(&u, &filename, gist_id.as_deref())?),
                    None => None,
                };
                if url.is_none() && gist_id.is_none() {
                    return Err(invalid("gist location needs `url` or `gist_id`"));
                }
                Ok(Location::Gist {
                    url,
                    gist_id,
                    filename,
                })
            }
            Location::Webdav {
                endpoint,
                path,
                user,
            } => {
                let mut ep = check_url(&endpoint, policy, "endpoint")?;
                if ep.query().is_some() {
                    return Err(invalid("endpoint must not contain a query"));
                }
                if !ep.path().ends_with('/') {
                    let p = format!("{}/", ep.path());
                    ep.set_path(&p);
                }
                let path = normalize_path(&path)?;
                if let Some(u) = &user {
                    if u.is_empty() || u.len() > 128 || u.chars().any(char::is_control) {
                        return Err(invalid("user must be 1..=128 printable characters"));
                    }
                }
                Ok(Location::Webdav {
                    endpoint: ep.to_string(),
                    path,
                    user,
                })
            }
            Location::Url { url } => {
                let u = check_url(&url, policy, "url")?;
                Ok(Location::Url { url: u.to_string() })
            }
        }
    }
}

/// Parsed import payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpec {
    pub stream_id: String,
    pub label: Option<String>,
    pub location: Location,
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    err(Stage::Config, "import_invalid", msg)
}

/// Field names that look like credentials. Matched case-insensitively as substrings.
const SECRET_MARKERS: &[&str] = &[
    "password",
    "passphrase",
    "passwd",
    "token",
    "secret",
    "key",
    "auth",
    "cookie",
    "credential",
];

/// Parse a pasted `rkpsync://` URI or raw JSON payload.
pub fn parse_import(input: &str, policy: UrlPolicy) -> Result<ImportSpec> {
    let input = input.trim();
    let json: Vec<u8> = if input.starts_with('{') {
        input.as_bytes().to_vec()
    } else if let Some(p) = input.strip_prefix(SCHEME_PREFIX) {
        if input.len() > limits::MAX_IMPORT_URI {
            return Err(invalid("import URI longer than 2 KiB"));
        }
        if !p
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(invalid(
                "`p` must be unpadded base64url and the only parameter",
            ));
        }
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(p)
            .map_err(|_| invalid("`p` is not valid base64url"))?
    } else if input.starts_with("rkpsync:") {
        return Err(invalid(
            "unsupported rkpsync URI; expected rkpsync://v1/import?p=…",
        ));
    } else {
        return Err(invalid("expected an rkpsync:// URI or a JSON object"));
    };
    if json.len() > limits::MAX_IMPORT_JSON {
        return Err(invalid("import JSON larger than 4 KiB"));
    }
    let value: Value =
        serde_json::from_slice(&json).map_err(|e| invalid(format!("import JSON: {e}")))?;
    let Value::Object(obj) = value else {
        return Err(invalid("import JSON must be an object"));
    };
    spec_from_object(obj, policy)
}

fn spec_from_object(mut obj: Map<String, Value>, policy: UrlPolicy) -> Result<ImportSpec> {
    for k in obj.keys() {
        let lower = k.to_ascii_lowercase();
        if SECRET_MARKERS.iter().any(|m| lower.contains(m)) {
            return Err(err(
                Stage::Config,
                "secret_field",
                format!("import payload must not carry credentials (field `{k}`)"),
            ));
        }
    }
    let provider = take_str(&mut obj, "provider")?.ok_or_else(|| invalid("missing `provider`"))?;
    let allowed: &[&str] = match provider.as_str() {
        "gist" => &["url", "gist_id", "filename"],
        "webdav" => &["endpoint", "path", "user"],
        "url" => &["url"],
        other => return Err(invalid(format!("unknown provider `{other}`"))),
    };
    const COMMON: &[&str] = &["v", "provider", "stream_id", "label", "format"];
    if let Some(k) = obj
        .keys()
        .find(|k| !COMMON.contains(&k.as_str()) && !allowed.contains(&k.as_str()))
    {
        return Err(err(
            Stage::Config,
            "unknown_field",
            format!("unknown field `{k}` for provider `{provider}`"),
        ));
    }
    match obj.remove("v") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        _ => return Err(invalid("`v` must be 1")),
    }
    match take_str(&mut obj, "format")?.as_deref() {
        Some(FORMAT) => {}
        _ => return Err(invalid("`format` must be \"age-armor\"")),
    }
    let stream_id =
        take_str(&mut obj, "stream_id")?.ok_or_else(|| invalid("missing `stream_id`"))?;
    if !valid_stream_id(&stream_id) {
        return Err(invalid("`stream_id` has invalid format"));
    }
    let label = take_str(&mut obj, "label")?;
    if let Some(l) = &label {
        check_label(l)?;
    }
    let location = match provider.as_str() {
        "gist" => Location::Gist {
            url: take_str(&mut obj, "url")?,
            gist_id: take_str(&mut obj, "gist_id")?,
            filename: take_str(&mut obj, "filename")?
                .unwrap_or_else(|| REMOTE_FILENAME.to_string()),
        },
        "webdav" => Location::Webdav {
            endpoint: take_str(&mut obj, "endpoint")?
                .ok_or_else(|| invalid("missing `endpoint`"))?,
            path: take_str(&mut obj, "path")?.ok_or_else(|| invalid("missing `path`"))?,
            user: take_str(&mut obj, "user")?,
        },
        _ => Location::Url {
            url: take_str(&mut obj, "url")?.ok_or_else(|| invalid("missing `url`"))?,
        },
    }
    .validated(policy)?;
    Ok(ImportSpec {
        stream_id,
        label,
        location,
    })
}

fn take_str(obj: &mut Map<String, Value>, k: &str) -> Result<Option<String>> {
    match obj.remove(k) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(invalid(format!("`{k}` must be a string"))),
    }
}

/// Encode an import spec as `(uri, json)`. Contains no credentials by construction.
pub fn encode_import(spec: &ImportSpec) -> (String, String) {
    let mut m = Map::new();
    m.insert("v".into(), Value::from(1));
    m.insert("provider".into(), Value::from(spec.location.provider()));
    match &spec.location {
        Location::Gist {
            url,
            gist_id,
            filename,
        } => {
            if let Some(u) = url {
                m.insert("url".into(), Value::from(u.as_str()));
            }
            if let Some(id) = gist_id {
                m.insert("gist_id".into(), Value::from(id.as_str()));
            }
            m.insert("filename".into(), Value::from(filename.as_str()));
        }
        Location::Webdav {
            endpoint,
            path,
            user,
        } => {
            m.insert("endpoint".into(), Value::from(endpoint.as_str()));
            m.insert("path".into(), Value::from(path.as_str()));
            if let Some(u) = user {
                m.insert("user".into(), Value::from(u.as_str()));
            }
        }
        Location::Url { url } => {
            m.insert("url".into(), Value::from(url.as_str()));
        }
    }
    m.insert("stream_id".into(), Value::from(spec.stream_id.as_str()));
    if let Some(l) = &spec.label {
        m.insert("label".into(), Value::from(l.as_str()));
    }
    m.insert("format".into(), Value::from(FORMAT));
    let json = Value::Object(m).to_string();
    let p = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes());
    (format!("{SCHEME_PREFIX}{p}"), json)
}

pub fn check_label(l: &str) -> Result<()> {
    if l.chars().count() > MAX_LABEL_CHARS || l.chars().any(char::is_control) {
        return Err(invalid("`label` must be <= 64 printable characters"));
    }
    Ok(())
}

/// `[A-Za-z0-9._-]+` ending in `.age`.
pub fn check_filename(f: &str) -> Result<()> {
    let ok = f.len() <= 128
        && f.len() > 4
        && f.ends_with(".age")
        && !f.starts_with('.')
        && f.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !ok {
        return Err(invalid(
            "filename must match [A-Za-z0-9._-]+ and end with .age",
        ));
    }
    Ok(())
}

pub fn check_gist_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(invalid("gist_id must be alphanumeric"));
    }
    Ok(())
}

/// Gist raw URL: `https://gist.githubusercontent.com/<owner>/<id>/raw/<filename>`.
/// Revision-pinned URLs (`…/raw/<rev>/<filename>`) are rejected.
pub fn check_gist_url(u: &str, filename: &str, gist_id: Option<&str>) -> Result<String> {
    let url = check_url(u, UrlPolicy::default(), "gist url")?;
    if url.host_str() != Some(GIST_RAW_HOST) || url.port().is_some() {
        return Err(invalid(format!("gist url host must be {GIST_RAW_HOST}")));
    }
    if url.query().is_some() {
        return Err(invalid("gist url must not contain a query"));
    }
    let segs: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
    match segs.as_slice() {
        [owner, id, "raw", file] => {
            if owner.is_empty()
                || owner.len() > 39
                || !owner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(invalid("gist url owner is invalid"));
            }
            check_gist_id(id)?;
            if gist_id.is_some_and(|g| g != *id) {
                return Err(invalid("gist url does not match gist_id"));
            }
            if *file != filename {
                return Err(invalid("gist url file name does not match filename"));
            }
        }
        [_, _, "raw", _, _] => {
            return Err(err(
                Stage::Config,
                "gist_revision_url",
                "gist url pins a revision; use the latest-version raw URL",
            ))
        }
        _ => return Err(invalid("gist url must be …/<owner>/<id>/raw/<filename>")),
    }
    Ok(url.to_string())
}

/// Common URL checks: scheme, userinfo, host, fragment, private-address policy.
pub fn check_url(s: &str, policy: UrlPolicy, what: &str) -> Result<Url> {
    if s.len() > 1024 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid(format!(
            "{what} is too long or contains whitespace"
        )));
    }
    let url = Url::parse(s).map_err(|e| invalid(format!("{what} is not a URL: {e}")))?;
    match url.scheme() {
        "https" => {}
        "http" if policy.allow_insecure_http => {}
        "http" => {
            return Err(err(
                Stage::Config,
                "insecure_url",
                format!("{what} must use https:// (http needs --allow-insecure-http)"),
            ))
        }
        other => return Err(invalid(format!("{what} scheme `{other}` is not allowed"))),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(err(
            Stage::Config,
            "url_userinfo",
            format!("{what} must not contain user:password@"),
        ));
    }
    if url.fragment().is_some() {
        return Err(invalid(format!("{what} must not contain a fragment")));
    }
    let host = url
        .host()
        .ok_or_else(|| invalid(format!("{what} has no host")))?;
    if !policy.allow_private && host_is_private(&host) {
        return Err(err(
            Stage::Config,
            "private_address",
            format!("{what} points at a local/private address (needs --allow-private)"),
        ));
    }
    Ok(url)
}

/// Literal-host check; DNS answers are checked again at connect time.
pub fn host_is_private(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || d.ends_with(".lan")
                || d.ends_with(".home.arpa")
                || !d.contains('.')
        }
        Host::Ipv4(ip) => ip_is_private(IpAddr::V4(*ip)),
        Host::Ipv6(ip) => ip_is_private(IpAddr::V6(*ip)),
    }
}

/// Loopback, private, link-local, CGNAT, unspecified, multicast, documentation, ULA.
pub fn ip_is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_private(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4_private(v4);
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xfe00) == 0xfc00 // fc00::/7 unique local
                || (seg0 & 0xffc0) == 0xfe80 // fe80::/10 link local
                || (seg0 == 0x2001 && v6.segments()[1] == 0x0db8) // documentation
                || v6 == Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0) // NAT64 prefix base
        }
    }
}

fn v4_private(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || v4.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
}

/// Normalise a WebDAV file path: absolute, no `.`/`..`/empty segments, no
/// control characters or `?#\`, last segment ends with `.age`.
pub fn normalize_path(p: &str) -> Result<String> {
    if p.len() > 512 || !p.starts_with('/') {
        return Err(invalid("path must be absolute and <= 512 bytes"));
    }
    if p.chars()
        .any(|c| c.is_control() || matches!(c, '?' | '#' | '\\' | '%'))
    {
        return Err(invalid("path contains forbidden characters"));
    }
    let segs: Vec<&str> = p.split('/').skip(1).collect();
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err(err(
            Stage::Config,
            "path_traversal",
            "path must not contain empty, `.` or `..` segments",
        ));
    }
    let last = segs.last().copied().unwrap_or_default();
    if !last.ends_with(".age") || last.len() <= 4 {
        return Err(invalid("path must name a file ending in .age"));
    }
    Ok(p.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_sync_error;

    const P: UrlPolicy = UrlPolicy {
        allow_insecure_http: false,
        allow_private: false,
    };

    fn code(r: Result<ImportSpec>) -> String {
        to_sync_error(&r.unwrap_err()).code
    }

    fn json_import(v: serde_json::Value) -> Result<ImportSpec> {
        parse_import(&v.to_string(), P)
    }

    #[test]
    fn accepts_three_providers_and_roundtrips() {
        let gist = json_import(serde_json::json!({
            "v": 1, "provider": "gist",
            "url": "https://gist.githubusercontent.com/owner/abc123/raw/rkp-sync.age",
            "gist_id": "abc123", "filename": "rkp-sync.age",
            "stream_id": "st_01JTESTSTREAM", "label": "phone-a", "format": "age-armor"
        }))
        .unwrap();
        let webdav = json_import(serde_json::json!({
            "v": 1, "provider": "webdav",
            "endpoint": "https://dav.jianguoyun.com/dav",
            "path": "/rkp/phone-a-keybox.age", "user": "me@example.com",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
        }))
        .unwrap();
        let url = json_import(serde_json::json!({
            "v": 1, "provider": "url", "url": "https://example.com/rkp/rkp-sync.age?dl=1",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
        }))
        .unwrap();
        for spec in [gist, webdav.clone(), url] {
            let (uri, json) = encode_import(&spec);
            assert!(uri.starts_with(SCHEME_PREFIX));
            assert!(uri.len() <= limits::MAX_IMPORT_URI);
            assert_eq!(parse_import(&uri, P).unwrap(), spec);
            assert_eq!(parse_import(&json, P).unwrap(), spec);
        }
        assert_eq!(
            webdav.location.webdav_file_url().unwrap().as_str(),
            "https://dav.jianguoyun.com/dav/rkp/phone-a-keybox.age"
        );
    }

    #[test]
    fn rejects_secret_and_unknown_fields() {
        for k in [
            "password",
            "passphrase",
            "token",
            "secret",
            "key",
            "api_key",
            "Password",
        ] {
            let mut v = serde_json::json!({
                "v": 1, "provider": "url", "url": "https://example.com/a.age",
                "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
            });
            v[k] = serde_json::json!("hunter2");
            assert_eq!(code(json_import(v)), "secret_field", "field {k}");
        }
        let v = serde_json::json!({
            "v": 1, "provider": "url", "url": "https://example.com/a.age",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor", "target": "/data/x"
        });
        assert_eq!(code(json_import(v)), "unknown_field");
        // Field valid for another provider is still unknown here.
        let v = serde_json::json!({
            "v": 1, "provider": "url", "url": "https://example.com/a.age", "endpoint": "https://x.com/",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
        });
        assert_eq!(code(json_import(v)), "unknown_field");
    }

    #[test]
    fn rejects_userinfo_http_and_private_hosts() {
        let mk = |url: &str| {
            serde_json::json!({
                "v": 1, "provider": "url", "url": url,
                "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
            })
        };
        assert_eq!(
            code(json_import(mk("https://u:p@example.com/a.age"))),
            "url_userinfo"
        );
        assert_eq!(
            code(json_import(mk("https://u@example.com/a.age"))),
            "url_userinfo"
        );
        assert_eq!(
            code(json_import(mk("http://example.com/a.age"))),
            "insecure_url"
        );
        for host in [
            "https://127.0.0.1/a.age",
            "https://localhost/a.age",
            "https://10.0.0.5/a.age",
            "https://192.168.1.2/a.age",
            "https://169.254.169.254/latest",
            "https://[::1]/a.age",
            "https://[fd00::1]/a.age",
            "https://[::ffff:127.0.0.1]/a.age",
            "https://100.64.0.1/a.age",
            "https://nas/a.age",
            "https://router.lan/a.age",
        ] {
            assert_eq!(code(json_import(mk(host))), "private_address", "{host}");
        }
        let dev = UrlPolicy {
            allow_insecure_http: true,
            allow_private: true,
        };
        assert!(parse_import(&mk("http://127.0.0.1:8080/a.age").to_string(), dev).is_ok());
        assert_eq!(
            code(json_import(mk("file:///etc/passwd"))),
            "import_invalid"
        );
    }

    #[test]
    fn rejects_bad_webdav_paths() {
        for (path, want) in [
            ("/rkp/../etc/x.age", "path_traversal"),
            ("/rkp//x.age", "path_traversal"),
            ("/rkp/./x.age", "path_traversal"),
            ("rkp/x.age", "import_invalid"),
            ("/rkp/x.txt", "import_invalid"),
            ("/rkp/x\u{0}.age", "import_invalid"),
            ("/rkp/%2e%2e/x.age", "import_invalid"),
        ] {
            let v = serde_json::json!({
                "v": 1, "provider": "webdav", "endpoint": "https://dav.example.com/dav/",
                "path": path, "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
            });
            assert_eq!(code(json_import(v)), want, "{path:?}");
        }
    }

    #[test]
    fn gist_url_rules() {
        let mk = |url: &str, filename: &str| {
            serde_json::json!({
                "v": 1, "provider": "gist", "url": url, "filename": filename,
                "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
            })
        };
        assert_eq!(
            code(json_import(mk(
                "https://gist.githubusercontent.com/o/abc/raw/0123abcd/rkp-sync.age",
                "rkp-sync.age"
            ))),
            "gist_revision_url"
        );
        assert_eq!(
            code(json_import(mk(
                "https://evil.example.com/o/abc/raw/rkp-sync.age",
                "rkp-sync.age"
            ))),
            "import_invalid"
        );
        assert_eq!(
            code(json_import(mk(
                "https://gist.githubusercontent.com/o/abc/raw/x.age",
                "../x.age"
            ))),
            "import_invalid"
        );
        // gist_id only (API fallback) is allowed.
        let v = serde_json::json!({
            "v": 1, "provider": "gist", "gist_id": "abc123",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor"
        });
        let s = json_import(v).unwrap();
        assert!(matches!(s.location, Location::Gist { url: None, .. }));
    }

    #[test]
    fn rejects_malformed_uri_and_limits() {
        assert_eq!(
            code(parse_import("rkpsync://v2/import?p=e30", P)),
            "import_invalid"
        );
        assert_eq!(
            code(parse_import("rkpsync://v1/import?p=e30&x=1", P)),
            "import_invalid"
        );
        assert_eq!(
            code(parse_import("rkpsync://v1/import?p=e30=", P)),
            "import_invalid"
        );
        let long = format!("{SCHEME_PREFIX}{}", "A".repeat(limits::MAX_IMPORT_URI));
        assert_eq!(code(parse_import(&long, P)), "import_invalid");
        let big = format!("{{\"label\":\"{}\"}}", "a".repeat(limits::MAX_IMPORT_JSON));
        assert_eq!(code(parse_import(&big, P)), "import_invalid");
        let v = serde_json::json!({
            "v": 1, "provider": "url", "url": "https://example.com/a.age",
            "stream_id": "st_01JTESTSTREAM", "format": "age-armor",
            "label": "x".repeat(65)
        });
        assert_eq!(code(json_import(v)), "import_invalid");
    }
}
