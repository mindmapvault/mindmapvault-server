//! Works out which address a request came from, for throttling purposes.
//!
//! Behind a reverse proxy every request arrives from the proxy, so the peer
//! address identifies the proxy rather than the caller and a per-address limit
//! would apply to the whole instance at once. `X-Forwarded-For` carries the
//! real address — but it is a request header, so anyone can write whatever they
//! like in it.
//!
//! This used to be a boolean, `trust_proxy_headers`, which took the **left-most**
//! entry when it was on. That entry is the one furthest from the server and
//! entirely chosen by the caller, so an instance behind a proxy — the only
//! deployment where the setting is any use — could be stepped around with
//! `X-Forwarded-For: <anything>` and a fresh allowance per request. Turning it
//! off meant every client behind the proxy shared one bucket instead. Neither
//! side of that switch was a working configuration.
//!
//! So the operator declares the proxies instead, by address range, and the
//! client is the first hop that is **not** one of them, counted from the server
//! outwards. Entries an attacker prepends sit further out than the real client
//! and are never reached. Declaring nothing trusts nothing and uses the peer
//! address, which is correct for a directly-exposed server.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRef, FromRequestParts},
    http::{request::Parts, HeaderMap},
};
use ipnet::IpNet;

use crate::models::instance_settings::InstanceSettingsHandle;

/// Address used to key the throttles.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl std::fmt::Display for ClientIp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The address ranges the operator runs their own proxies on.
///
/// Empty means "no proxy in front of this server", which is the default and
/// the safe reading of an unconfigured instance: `X-Forwarded-For` is then
/// never consulted at all.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies(Vec<IpNet>);

impl TrustedProxies {
    /// Parses the configured entries, dropping any that are not valid.
    ///
    /// A bare address is accepted as a single-host range, because `10.0.0.5` is
    /// what an operator will write when they mean `10.0.0.5/32`.
    ///
    /// Returns the parsed list alongside the entries it could not read, so the
    /// caller can say so at startup rather than silently trusting less than the
    /// operator asked for.
    pub fn parse<I, S>(entries: I) -> (Self, Vec<String>)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut nets = Vec::new();
        let mut rejected = Vec::new();

        for entry in entries {
            let raw = entry.as_ref().trim();
            if raw.is_empty() {
                continue;
            }
            if let Ok(net) = raw.parse::<IpNet>() {
                nets.push(net);
            } else if let Ok(address) = raw.parse::<IpAddr>() {
                nets.push(IpNet::from(address));
            } else {
                rejected.push(raw.to_string());
            }
        }

        (Self(nets), rejected)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        self.0.iter().any(|net| net.contains(&address))
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
        let (trusted, _) = TrustedProxies::parse(settings.trusted_proxy_cidrs.iter());
        Ok(resolve(&parts.headers, peer_address(parts), &trusted))
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
/// Walks the hop chain from the server outwards — the peer first, then
/// `X-Forwarded-For` right to left — and returns the first hop that is not a
/// declared proxy. Anything further out than that is either a real upstream we
/// were not told about or a forgery, and either way must not be believed.
pub fn resolve(headers: &HeaderMap, peer: Option<IpAddr>, trusted: &TrustedProxies) -> ClientIp {
    // No peer address means the server was mounted without connection info.
    // Falling back to a fixed address keeps the limiter switched on (as one
    // shared bucket) rather than quietly letting everything through.
    let peer = peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    // Nothing declared: there is no proxy to speak for anyone else.
    if trusted.is_empty() || !trusted.contains(peer) {
        return ClientIp(peer);
    }

    // The peer is one of ours, so it is forwarding for somebody. `nearest` is
    // the innermost hop known to be trusted, and the answer if the chain runs
    // out or stops making sense.
    let mut nearest = peer;
    for entry in forwarded_entries(headers).into_iter().rev() {
        match entry {
            Some(address) if trusted.contains(address) => nearest = address,
            Some(address) => return ClientIp(address),
            // An entry that will not parse cannot be judged, and believing
            // anything beyond it would mean trusting the part of the header a
            // forger controls. Stop at the last hop we could vouch for.
            None => return ClientIp(nearest),
        }
    }

    ClientIp(nearest)
}

/// Every `X-Forwarded-For` entry in header order, `None` where one could not be
/// read. Positions are preserved: dropping the unreadable ones would let a
/// forged entry shift the chain.
fn forwarded_entries(headers: &HeaderMap) -> Vec<Option<IpAddr>> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(parse_address)
        .collect()
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

    fn trusting(entries: &[&str]) -> TrustedProxies {
        let (trusted, rejected) = TrustedProxies::parse(entries.iter());
        assert!(rejected.is_empty(), "test fixture should parse: {rejected:?}");
        trusted
    }

    #[test]
    fn nothing_declared_means_the_header_is_never_consulted() {
        let resolved = resolve(&headers_with("1.2.3.4"), peer(), &TrustedProxies::default());
        assert_eq!(resolved.0.to_string(), "10.0.0.5");
    }

    #[test]
    fn a_declared_proxy_speaks_for_the_client_behind_it() {
        let resolved = resolve(&headers_with("1.2.3.4"), peer(), &trusting(&["10.0.0.0/8"]));
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_prepended_entry_cannot_displace_the_real_client() {
        // The forgery sits further out than the address the proxy appended, so
        // the walk inwards never reaches it. This is the bypass that the old
        // left-most reading allowed.
        let resolved = resolve(
            &headers_with("9.9.9.9, 1.2.3.4"),
            peer(),
            &trusting(&["10.0.0.0/8"]),
        );
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_chain_of_declared_proxies_is_walked_through() {
        let resolved = resolve(
            &headers_with("1.2.3.4, 10.0.0.9, 10.0.0.7"),
            peer(),
            &trusting(&["10.0.0.0/8"]),
        );
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn an_undeclared_proxy_is_treated_as_the_client() {
        // Better to throttle the proxy as one bucket than to believe a header
        // from a hop the operator never vouched for.
        let resolved = resolve(&headers_with("1.2.3.4"), peer(), &trusting(&["192.168.0.0/16"]));
        assert_eq!(resolved.0.to_string(), "10.0.0.5");
    }

    #[test]
    fn an_exhausted_chain_falls_back_to_the_innermost_proxy() {
        let resolved = resolve(&headers_with("10.0.0.9"), peer(), &trusting(&["10.0.0.0/8"]));
        assert_eq!(resolved.0.to_string(), "10.0.0.9");
    }

    #[test]
    fn a_header_with_no_entries_falls_back_to_the_peer() {
        let resolved = resolve(&HeaderMap::new(), peer(), &trusting(&["10.0.0.0/8"]));
        assert_eq!(resolved.0.to_string(), "10.0.0.5");
    }

    #[test]
    fn junk_stops_the_walk_rather_than_being_skipped_over() {
        // Skipping it would let a forger insert junk to push the chain along
        // and have an entry they wrote read as the client.
        let resolved = resolve(
            &headers_with("1.2.3.4, not-an-address, 10.0.0.9"),
            peer(),
            &trusting(&["10.0.0.0/8"]),
        );
        assert_eq!(resolved.0.to_string(), "10.0.0.9");
    }

    #[test]
    fn a_port_suffix_is_stripped() {
        let resolved = resolve(&headers_with("1.2.3.4:51000"), peer(), &trusting(&["10.0.0.0/8"]));
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_bracketed_ipv6_entry_is_understood() {
        let resolved = resolve(
            &headers_with("[2001:db8::1]:443"),
            peer(),
            &trusting(&["10.0.0.0/8"]),
        );
        assert_eq!(resolved.0.to_string(), "2001:db8::1");
    }

    #[test]
    fn the_header_may_arrive_split_across_several_lines() {
        let mut headers = HeaderMap::new();
        headers.append("x-forwarded-for", "1.2.3.4".parse().expect("valid"));
        headers.append("x-forwarded-for", "10.0.0.9".parse().expect("valid"));
        let resolved = resolve(&headers, peer(), &trusting(&["10.0.0.0/8"]));
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn an_ipv6_proxy_range_is_matched() {
        let peer6 = Some("2001:db8::99".parse::<IpAddr>().expect("valid"));
        let resolved = resolve(&headers_with("1.2.3.4"), peer6, &trusting(&["2001:db8::/32"]));
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn a_bare_address_is_read_as_a_single_host_range() {
        let resolved = resolve(&headers_with("1.2.3.4"), peer(), &trusting(&["10.0.0.5"]));
        assert_eq!(resolved.0.to_string(), "1.2.3.4");
    }

    #[test]
    fn unreadable_entries_are_reported_rather_than_dropped_in_silence() {
        let (trusted, rejected) = TrustedProxies::parse(["10.0.0.0/8", "haproxy.internal", ""]);
        assert!(trusted.contains("10.1.2.3".parse().expect("valid")));
        assert_eq!(rejected, vec!["haproxy.internal".to_string()]);
    }
}
