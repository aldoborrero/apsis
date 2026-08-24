//! Reverse-proxy authentication (spec 006 T004). The console is meant to sit behind an
//! auth proxy (oauth2-proxy / Traefik forward-auth) that injects the authenticated user as
//! a header. We do not authenticate ourselves — we only require that the proxy has, and
//! surface the identity for control-action gating.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;

/// The two header names auth proxies commonly set for the authenticated user.
const USER_HEADERS: [&str; 2] = ["x-forwarded-user", "x-auth-request-user"];

/// The authenticated operator, taken from the reverse-proxy user header. Extracting it fails
/// with `401` when no such header is present — i.e. the request did not pass the auth proxy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyUser(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ProxyUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        for name in USER_HEADERS {
            if let Some(v) = parts.headers.get(name).and_then(|v| v.to_str().ok()) {
                let v = v.trim();
                if !v.is_empty() {
                    return Ok(ProxyUser(v.to_string()));
                }
            }
        }
        Err((
            StatusCode::UNAUTHORIZED,
            "missing reverse-proxy user header",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    async fn extract(headers: &[(&str, &str)]) -> Result<ProxyUser, (StatusCode, &'static str)> {
        let mut req = Request::builder();
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let (mut parts, _) = req.body(()).unwrap().into_parts();
        ProxyUser::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn present_header_yields_user() {
        assert_eq!(
            extract(&[("x-forwarded-user", "alice")]).await,
            Ok(ProxyUser("alice".into()))
        );
        assert_eq!(
            extract(&[("x-auth-request-user", "bob")]).await,
            Ok(ProxyUser("bob".into()))
        );
    }

    #[tokio::test]
    async fn absent_or_empty_header_is_unauthorized() {
        assert!(extract(&[]).await.is_err());
        assert!(extract(&[("x-forwarded-user", "  ")]).await.is_err());
    }
}
