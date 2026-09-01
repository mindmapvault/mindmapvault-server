//! Works out which address a request came from, for throttling purposes.
//!
//! Behind a reverse proxy every request arrives from the proxy, so the peer
//! address identifies the proxy rather than the caller and a per-address limit
//! would apply to the whole instance at once. `X-Forwarded-For` carries the
//! real address — but it is a request header, so anyone can write whatever they
//! like in it, and trusting it on a directly-exposed server hands an attacker
//! an unlimited supply of identities.
//!
//! There is no way to tell the two deployments apart from inside the process,
//! so the operator says which one it is (`trust_proxy_headers`). The default is
//! not to trust the header: a throttle that is too coarse is a visible problem,
//! while one that is silently bypassed is not.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRef, FromRequestParts},
    http::{request::Parts, HeaderMap},
};

use crate::models::instance_settings::InstanceSettingsHandle;

/// Address used to key the throttles.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl std::fmt::Display for ClientIp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
    InstanceSettingsHandle: FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let settings = <InstanceSettingsHandle as FromRef<S>>::from_ref(state).get();
        Ok(resolve(&parts.headers, peer_address(parts), settings.trust_proxy_headers))
    }
}

fn peer_address(parts: &Parts) -> Option<IpAddr> {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

/// Picks the address to throttle on.
///
/// When proxy headers are trusted, the **left-most** `X-Forwarded-For` entry is
/// the original client — the one a well-behaved proxy chain appends to. That
/// entry is also the one a caller can forge, which is exactly why this is off
/// unless the operator turned it on.
pub fn resolve(headers: &HeaderMap, peer: Option<IpAddr>, trust_proxy_headers: bool) -> ClientIp {
    if trust_proxy_headers {
        if let Some(address) = forwarded_for(headers) {
            return ClientIp(address);
        }
    }

    // No peer address means the server was mounted without connection info.
    // Falling back to a fixed address keeps the limiter switched on (as one
    // shared bucket) rather than quietly letting everything through.
    ClientIp(peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
}

fn forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let raw = headers.get("x-forwarded-for")?.to_str().ok()?;
    raw.split(',')
        .map(str::trim)
        .find(|entry| !entry.is_empty())
        .and_then(parse_address)
}

/// Accepts both bare addresses and the `address:port` form some proxies emit,
/// including bracketed IPv6.
fn parse_address(entry: &str) -> Option<IpAddr> {
    if let Ok(address) = entry.parse::<IpAddr>() {
        return Some(address);
    }
    if let Ok(socket) = entry.parse::<SocketAddr>() {
        return Some(socket.ip());
    }
    entry
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .and_then(|inner| inner.parse::<IpAddr>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().expect("valid header"));
        headers
    }

    fn peer() -> Option<IpAddr> {
        Some("10.0.0.5".parse().expect("valid address"))
    }

    #[test]
    fn a_spoofed_header_is_ignored_by_default() {
        let resolved = resolve(&headers_with("1.2.3.4"), peer(), false);
        assert_eq!(resolved.0.to_string(), "10.0.0.5");
    }

    #[test]
    fn the_original_client_is_taken_when_proxies_are_trusted() {
        let resolved = resolve(&headers_with("1.2.3.4, 10.0.0.5"), peer(), true);
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_port_suffix_is_stripped() {
        let resolved = resolve(&headers_with("1.2.3.4:51000"), peer(), true);
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_bracketed_ipv6_entry_is_understood() {
        let resolved = resolve(&headers_with("[2001:db8::1]:443"), peer(), true);
        assert_eq!(resolved.0.to_string(), "2001:db8::1");
    }

    #[test]
    fn a_junk_header_falls_back_to_the_peer_rather_than_opening_the_gate() {
        let resolved = resolve(&headers_with("not-an-address"), peer(), true);
        assert_eq!(resolved.0.to_string(), "10.0.0.5");
    }
}
