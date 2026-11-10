//! Blocking HTTPS client with the sync URL policy enforced at every hop.
//!
//! - Only `https://` (plain `http://` only with `--allow-insecure-http`).
//! - DNS answers pointing at loopback/private/link-local addresses are
//!   rejected unless `--allow-private` (defends against DNS rebinding; the
//!   literal host is checked by `locator::check_url` first). The configured
//!   proxy, if any, is exempt because users commonly run it on 127.0.0.1.
//! - Redirects are followed manually (max 3), only to the same scheme, host
//!   and port; credentials are re-sent only under that rule.
//! - Bodies are capped at `limits::MAX_RESPONSE` (Content-Length checked
//!   first, then a bounded read).
//! - Request/response bodies and auth headers are never logged.

use std::io::{self, Read};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::limits;
use rkp_mini_core::locator::{self, UrlPolicy};
use url::Url;

const MAX_REDIRECTS: usize = 3;

pub struct Http {
    agent: ureq::Agent,
    policy: UrlPolicy,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: Vec<u8>,
}

impl Response {
    /// Best available change token: ETag, else Last-Modified.
    pub fn token(&self) -> Option<String> {
        self.etag.clone().or_else(|| self.last_modified.clone())
    }
}

struct GuardedResolver {
    allow_private: bool,
    proxy_netloc: Option<String>,
}

impl ureq::Resolver for GuardedResolver {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
        if self.allow_private || self.proxy_netloc.as_deref() == Some(netloc) {
            return Ok(addrs);
        }
        let public: Vec<SocketAddr> = addrs
            .into_iter()
            .filter(|a| !locator::ip_is_private(a.ip()))
            .collect();
        if public.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host resolves only to local/private addresses (needs --allow-private)",
            ));
        }
        Ok(public)
    }
}

fn proxy_netloc(proxy: &str) -> Option<String> {
    let u = Url::parse(proxy).ok()?;
    Some(format!("{}:{}", u.host_str()?, u.port_or_known_default()?))
}

impl Http {
    pub fn new(timeout_secs: u64, proxy: &str, policy: UrlPolicy) -> Result<Http> {
        let mut b = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout_secs))
            .redirects(0)
            .https_only(!policy.allow_insecure_http)
            .user_agent(concat!("rkp-sync/", env!("CARGO_PKG_VERSION")))
            .resolver(GuardedResolver {
                allow_private: policy.allow_private,
                proxy_netloc: if proxy.is_empty() {
                    None
                } else {
                    proxy_netloc(proxy)
                },
            });
        if !proxy.is_empty() {
            let p = ureq::Proxy::new(proxy)
                .map_err(|e| err(Stage::Config, "config_invalid", format!("proxy: {e}")))?;
            b = b.proxy(p);
        }
        Ok(Http {
            agent: b.build(),
            policy,
        })
    }

    /// Perform a request; non-2xx statuses are returned, not raised.
    pub fn send(
        &self,
        method: &str,
        url: &Url,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<Response> {
        let mut current = url.clone();
        for _ in 0..=MAX_REDIRECTS {
            locator::check_url(current.as_str(), self.policy, "request url")?;
            let mut req = self.agent.request_url(method, &current);
            for (k, v) in headers {
                req = req.set(k, v);
            }
            let result = match body {
                Some(b) => req.send_bytes(b),
                None => req.call(),
            };
            let resp = match result {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => r,
                Err(ureq::Error::Transport(t)) => {
                    // Transport messages carry host/IO detail only, never bodies.
                    return Err(err(
                        Stage::Remote,
                        "network",
                        format!("{method} {}: {t}", redact(&current)),
                    ));
                }
            };
            let status = resp.status();
            if (300..400).contains(&status) && status != 304 {
                let loc = resp.header("Location").ok_or_else(|| {
                    err(Stage::Remote, "bad_redirect", "redirect without Location")
                })?;
                let next = current
                    .join(loc)
                    .map_err(|_| err(Stage::Remote, "bad_redirect", "invalid redirect target"))?;
                if next.scheme() != current.scheme()
                    || next.host_str() != current.host_str()
                    || next.port_or_known_default() != current.port_or_known_default()
                {
                    return Err(err(
                        Stage::Remote,
                        "cross_host_redirect",
                        format!(
                            "refusing redirect from {} to another host/scheme",
                            redact(&current)
                        ),
                    ));
                }
                current = next;
                continue;
            }
            return read_response(resp);
        }
        Err(err(
            Stage::Remote,
            "too_many_redirects",
            "too many redirects",
        ))
    }
}

fn read_response(resp: ureq::Response) -> Result<Response> {
    let status = resp.status();
    let etag = resp.header("ETag").map(str::to_string);
    let last_modified = resp.header("Last-Modified").map(str::to_string);
    if let Some(len) = resp
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > limits::MAX_RESPONSE as u64 {
            return Err(err(
                Stage::Remote,
                "response_too_large",
                format!("response is {len} bytes; limit is 256 KiB"),
            ));
        }
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take(limits::MAX_RESPONSE as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| err(Stage::Remote, "network", format!("read body: {e}")))?;
    if body.len() > limits::MAX_RESPONSE {
        return Err(err(
            Stage::Remote,
            "response_too_large",
            "response larger than 256 KiB",
        ));
    }
    Ok(Response {
        status,
        etag,
        last_modified,
        body,
    })
}

/// URL for diagnostics: scheme, host, path; no query (may carry share keys).
pub fn redact(u: &Url) -> String {
    format!(
        "{}://{}{}",
        u.scheme(),
        u.host_str().unwrap_or(""),
        u.path()
    )
}

/// Map common HTTP failure statuses to stable error codes (§14).
pub fn status_error(status: u16, what: &str) -> anyhow::Error {
    let (code, hint) = match status {
        401 | 403 => (
            "auth_failed",
            "authentication rejected; update the token/app password",
        ),
        404 => ("remote_missing", "not found"),
        409 => ("remote_conflict", "conflict; retry next round"),
        412 => (
            "remote_conflict",
            "precondition failed (changed concurrently)",
        ),
        423 => ("remote_locked", "resource locked; retry next round"),
        429 => ("rate_limited", "rate limited; retry later"),
        507 => ("quota_exceeded", "storage quota exceeded"),
        500..=599 => ("remote_unavailable", "server error; retry later"),
        _ => ("http_status", "unexpected status"),
    };
    err(
        Stage::Remote,
        code,
        format!("{what}: HTTP {status}: {hint}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// One-shot HTTP/1.1 server replying with `responses` in order.
    fn serve(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for r in responses {
                let (mut s, _) = l.accept().unwrap();
                let mut reader = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                seen.push(first.trim().to_string());
                s.write_all(r.as_bytes()).unwrap();
            }
            seen
        });
        (format!("http://{addr}"), h)
    }

    fn dev() -> UrlPolicy {
        UrlPolicy {
            allow_insecure_http: true,
            allow_private: true,
        }
    }

    #[test]
    fn follows_same_host_redirect_and_reads_etag() {
        let (base, h) = serve(vec![
            "HTTP/1.1 302 Found\r\nLocation: /b.age\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".into(),
        ]);
        let http = Http::new(5, "", dev()).unwrap();
        let r = http
            .send(
                "GET",
                &Url::parse(&format!("{base}/a.age")).unwrap(),
                &[],
                None,
            )
            .unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hi");
        assert_eq!(r.token().as_deref(), Some("\"v1\""));
        assert_eq!(
            h.join().unwrap(),
            vec!["GET /a.age HTTP/1.1", "GET /b.age HTTP/1.1"]
        );
    }

    #[test]
    fn refuses_cross_host_redirect() {
        let (base, h) = serve(vec![
            "HTTP/1.1 302 Found\r\nLocation: https://evil.example.com/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ]);
        let http = Http::new(5, "", dev()).unwrap();
        let e = http
            .send(
                "GET",
                &Url::parse(&format!("{base}/a.age")).unwrap(),
                &[],
                None,
            )
            .unwrap_err();
        assert_eq!(
            rkp_mini_core::error::to_sync_error(&e).code,
            "cross_host_redirect"
        );
        h.join().unwrap();
    }

    #[test]
    fn rejects_oversize_body() {
        let big = limits::MAX_RESPONSE + 1;
        let (base, h) = serve(vec![format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {big}\r\nConnection: close\r\n\r\n"
        )]);
        let http = Http::new(5, "", dev()).unwrap();
        let e = http
            .send(
                "GET",
                &Url::parse(&format!("{base}/a.age")).unwrap(),
                &[],
                None,
            )
            .unwrap_err();
        assert_eq!(
            rkp_mini_core::error::to_sync_error(&e).code,
            "response_too_large"
        );
        h.join().unwrap();
    }

    #[test]
    fn private_addresses_blocked_by_default() {
        let http = Http::new(5, "", UrlPolicy::default()).unwrap();
        let e = http
            .send(
                "GET",
                &Url::parse("https://127.0.0.1/a.age").unwrap(),
                &[],
                None,
            )
            .unwrap_err();
        assert_eq!(
            rkp_mini_core::error::to_sync_error(&e).code,
            "private_address"
        );
        let e = http
            .send(
                "GET",
                &Url::parse("http://example.com/a.age").unwrap(),
                &[],
                None,
            )
            .unwrap_err();
        assert_eq!(rkp_mini_core::error::to_sync_error(&e).code, "insecure_url");
        // DNS-level guard: a public-looking name resolving to loopback.
        let r = GuardedResolver {
            allow_private: false,
            proxy_netloc: None,
        };
        assert!(ureq::Resolver::resolve(&r, "localhost:443").is_err());
        let r = GuardedResolver {
            allow_private: false,
            proxy_netloc: Some("localhost:7890".into()),
        };
        assert!(ureq::Resolver::resolve(&r, "localhost:7890").is_ok());
    }
}
