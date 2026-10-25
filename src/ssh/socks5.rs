//! SOCKS5 wire-format helpers shared by the -D dynamic forward (server side)
//! and the connect-via-SOCKS5-proxy client (see `proxy.rs`).
//!
//! Only what RFC 1928 needs for Portal's use cases is implemented:
//! method negotiation (no-auth, optional user/pass for the client side),
//! the CONNECT command, and the three address forms (IPv4 / domain / IPv6).
//! Parsing is chunk-driven: buffers may arrive split at any byte boundary,
//! so every parser returns `SocksError::Incomplete` until it has a whole
//! message and never reads past its input.

/// SOCKS protocol version 5
pub const SOCKS_VERSION: u8 = 0x05;
/// No-authentication-required method
pub const METHOD_NO_AUTH: u8 = 0x00;
/// Username/password method (sub-negotiation per RFC 1929). The -D server
/// only offers no-auth; this is used by the connect-via-SOCKS5-proxy client
/// (see `proxy.rs`).
#[allow(dead_code)]
pub const METHOD_USER_PASS: u8 = 0x02;
/// No acceptable methods
pub const METHOD_NONE_ACCEPTABLE: u8 = 0xFF;
/// CONNECT command
pub const CMD_CONNECT: u8 = 0x01;
/// Command not supported reply code
pub const REP_CMD_NOT_SUPPORTED: u8 = 0x07;
/// Network unreachable (sent when the SSH channel open fails)
pub const REP_NETWORK_UNREACHABLE: u8 = 0x03;
/// General SOCKS server failure
pub const REP_GENERAL_FAILURE: u8 = 0x01;
/// Address type: IPv4
pub const ATYP_IPV4: u8 = 0x01;
/// Address type: fully-qualified domain name
pub const ATYP_DOMAIN: u8 = 0x03;
/// Address type: IPv6
pub const ATYP_IPV6: u8 = 0x04;

/// Parse errors (client side also reuses these for its own responses)
#[derive(Debug, Clone, PartialEq)]
pub enum SocksError {
    /// Need more bytes to decide — caller must read again and re-parse
    /// from the same buffer start (parsers are prefix-consuming only on
    /// success).
    Incomplete,
    UnsupportedVersion(u8),
    UnsupportedMethod,
    UnsupportedCommand(u8),
    Malformed,
}

impl std::fmt::Display for SocksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocksError::Incomplete => write!(f, "incomplete SOCKS5 message"),
            SocksError::UnsupportedVersion(v) => write!(f, "unsupported SOCKS version {}", v),
            SocksError::UnsupportedMethod => write!(f, "no acceptable auth method"),
            SocksError::UnsupportedCommand(c) => write!(f, "unsupported SOCKS command {:#04x}", c),
            SocksError::Malformed => write!(f, "malformed SOCKS5 message"),
        }
    }
}

/// Destination requested by a SOCKS5 CONNECT
#[derive(Debug, Clone, PartialEq)]
pub enum SocksHost {
    Ipv4(std::net::Ipv4Addr),
    Ipv6(std::net::Ipv6Addr),
    Domain(String),
}

impl std::fmt::Display for SocksHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocksHost::Ipv4(a) => write!(f, "{}", a),
            SocksHost::Ipv6(a) => write!(f, "{}", a),
            SocksHost::Domain(d) => write!(f, "{}", d),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SocksRequest {
    pub host: SocksHost,
    pub port: u16,
}

/// Parse a client greeting: `VER NMETHODS METHODS...`.
/// Returns the number of bytes consumed on success.
///
/// `supported` is the set of methods the local end is willing to accept
/// (the -D server passes just `{METHOD_NO_AUTH}`; the proxy client passes
/// the methods it offered). If none overlap, `UnsupportedMethod` is
/// returned together with the full greeting length so the caller can
/// still reply `0x05 0xFF` and drain the connection cleanly.
pub fn parse_greeting(buf: &[u8], supported: &[u8]) -> Result<(usize, u8), SocksError> {
    if buf.len() < 2 {
        return Err(SocksError::Incomplete);
    }
    if buf[0] != SOCKS_VERSION {
        return Err(SocksError::UnsupportedVersion(buf[0]));
    }
    let nmethods = buf[1] as usize;
    if nmethods == 0 {
        return Err(SocksError::Malformed);
    }
    let total = 2 + nmethods;
    if buf.len() < total {
        return Err(SocksError::Incomplete);
    }
    let chosen = buf[2..total]
        .iter()
        .find(|m| supported.contains(m))
        .copied();
    match chosen {
        Some(m) => Ok((total, m)),
        None => Err(SocksError::UnsupportedMethod),
    }
}

/// Parse a CONNECT request: `VER CMD RSV ATYP ADDR PORT`.
/// Returns the bytes consumed and the parsed request.
/// Only CMD_CONNECT is accepted.
pub fn parse_request(buf: &[u8]) -> Result<(usize, SocksRequest), SocksError> {
    if buf.len() < 4 {
        return Err(SocksError::Incomplete);
    }
    if buf[0] != SOCKS_VERSION {
        return Err(SocksError::UnsupportedVersion(buf[0]));
    }
    if buf[1] != CMD_CONNECT {
        return Err(SocksError::UnsupportedCommand(buf[1]));
    }
    let atyp = buf[3];
    match atyp {
        ATYP_IPV4 => {
            const LEN: usize = 4 + 4 + 2;
            if buf.len() < LEN {
                return Err(SocksError::Incomplete);
            }
            let host = std::net::Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
            let port = u16::from_be_bytes([buf[8], buf[9]]);
            Ok((LEN, SocksRequest { host: SocksHost::Ipv4(host), port }))
        }
        ATYP_DOMAIN => {
            if buf.len() < 5 {
                return Err(SocksError::Incomplete);
            }
            let dlen = buf[4] as usize;
            let total = 4 + 1 + dlen + 2;
            if buf.len() < total {
                return Err(SocksError::Incomplete);
            }
            let domain = std::str::from_utf8(&buf[5..5 + dlen])
                .map_err(|_| SocksError::Malformed)?
                .to_string();
            if domain.is_empty() {
                return Err(SocksError::Malformed);
            }
            let port = u16::from_be_bytes([buf[5 + dlen], buf[6 + dlen]]);
            Ok((total, SocksRequest { host: SocksHost::Domain(domain), port }))
        }
        ATYP_IPV6 => {
            const LEN: usize = 4 + 16 + 2;
            if buf.len() < LEN {
                return Err(SocksError::Incomplete);
            }
            let octets: [u8; 16] = buf[4..20].try_into().map_err(|_| SocksError::Malformed)?;
            let port = u16::from_be_bytes([buf[20], buf[21]]);
            Ok((LEN, SocksRequest { host: SocksHost::Ipv6(std::net::Ipv6Addr::from(octets)), port }))
        }
        _ => Err(SocksError::Malformed),
    }
}

/// Server reply to a greeting: accept the no-auth method.
pub fn greeting_reply() -> [u8; 2] {
    [SOCKS_VERSION, METHOD_NO_AUTH]
}

/// Server reply rejecting every offered method.
pub fn greeting_reject_reply() -> [u8; 2] {
    [SOCKS_VERSION, METHOD_NONE_ACCEPTABLE]
}

/// Encode a success reply with the bound address echoed back.
pub fn success_reply(host: &SocksHost, port: u16) -> Vec<u8> {
    let mut out = vec![SOCKS_VERSION, 0x00, 0x00];
    match host {
        SocksHost::Ipv4(a) => {
            out.push(ATYP_IPV4);
            out.extend_from_slice(&a.octets());
        }
        SocksHost::Ipv6(a) => {
            out.push(ATYP_IPV6);
            out.extend_from_slice(&a.octets());
        }
        SocksHost::Domain(d) => {
            out.push(ATYP_DOMAIN);
            out.push(d.len() as u8);
            out.extend_from_slice(d.as_bytes());
        }
    }
    out.extend_from_slice(&port.to_be_bytes());
    out
}

/// Encode a failure reply with the RFC-required placeholder 0.0.0.0:0.
pub fn failure_reply(code: u8) -> Vec<u8> {
    vec![
        SOCKS_VERSION, code, 0x00, ATYP_IPV4, 0, 0, 0, 0, 0, 0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_ok_no_auth() {
        let (n, method) = parse_greeting(&[0x05, 0x01, 0x00], &[METHOD_NO_AUTH]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(method, METHOD_NO_AUTH);
    }

    #[test]
    fn greeting_picks_first_supported_of_many() {
        let buf = [0x05, 0x03, 0x01, 0x02, 0x00];
        let (n, method) =
            parse_greeting(&buf, &[METHOD_NO_AUTH, METHOD_USER_PASS]).unwrap();
        assert_eq!(n, 5);
        // 0x02 appears before 0x00 in the offer
        assert_eq!(method, METHOD_USER_PASS);
    }

    #[test]
    fn greeting_rejects_when_no_overlap() {
        let err = parse_greeting(&[0x05, 0x01, 0x02], &[METHOD_NO_AUTH]).unwrap_err();
        assert_eq!(err, SocksError::UnsupportedMethod);
    }

    #[test]
    fn greeting_incomplete_then_complete() {
        // NMETHODS=2 but only the first method byte has arrived
        let full = [0x05, 0x02, 0x00, 0x01];
        assert_eq!(
            parse_greeting(&full[..3], &[0x00]),
            Err(SocksError::Incomplete)
        );
        let (n, method) = parse_greeting(&full, &[0x00]).unwrap();
        assert_eq!(n, 4);
        assert_eq!(method, METHOD_NO_AUTH);
    }

    #[test]
    fn greeting_bad_version() {
        assert_eq!(
            parse_greeting(&[0x04, 0x01, 0x00], &[0x00]),
            Err(SocksError::UnsupportedVersion(0x04))
        );
    }

    #[test]
    fn request_ipv4() {
        let buf = [0x05, 0x01, 0x00, 0x01, 10, 0, 0, 1, 0x1F, 0x90];
        let (n, req) = parse_request(&buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(
            req,
            SocksRequest { host: SocksHost::Ipv4(std::net::Ipv4Addr::new(10, 0, 0, 1)), port: 8080 }
        );
    }

    #[test]
    fn request_domain() {
        let dom = b"example.com";
        let mut buf = vec![0x05, 0x01, 0x00, ATYP_DOMAIN, dom.len() as u8];
        buf.extend_from_slice(dom);
        buf.extend_from_slice(&443u16.to_be_bytes());
        let (n, req) = parse_request(&buf).unwrap();
        assert_eq!(n, buf.len());
        assert_eq!(req.host, SocksHost::Domain("example.com".into()));
        assert_eq!(req.port, 443);
    }

    #[test]
    fn request_ipv6() {
        let mut buf = vec![0x05, 0x01, 0x00, ATYP_IPV6];
        buf.extend_from_slice(&[0x20; 16]);
        buf.extend_from_slice(&22u16.to_be_bytes());
        let (n, req) = parse_request(&buf).unwrap();
        assert_eq!(n, 22);
        assert_eq!(req.host, SocksHost::Ipv6(std::net::Ipv6Addr::from([0x20; 16])));
        assert_eq!(req.port, 22);
    }

    #[test]
    fn request_split_across_chunks_is_incomplete() {
        let full = [
            0x05_u8, 0x01, 0x00, ATYP_DOMAIN, 4, b'a', b'b', b'c', b'd', 0x00, 0x50,
        ];
        for cut in 1..full.len() {
            assert_eq!(
                parse_request(&full[..cut]),
                Err(SocksError::Incomplete),
                "prefix of length {} should be incomplete",
                cut
            );
        }
        assert!(parse_request(&full).is_ok());
    }

    #[test]
    fn request_rejects_bind_and_udp() {
        assert_eq!(
            parse_request(&[0x05, 0x02, 0x00, 0x01, 0, 0, 0, 0, 0, 0]),
            Err(SocksError::UnsupportedCommand(0x02))
        );
        assert_eq!(
            parse_request(&[0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0]),
            Err(SocksError::UnsupportedCommand(0x03))
        );
    }

    #[test]
    fn reply_layouts() {
        assert_eq!(greeting_reply(), [0x05, 0x00]);
        assert_eq!(greeting_reject_reply(), [0x05, 0xFF]);
        let rep = success_reply(&SocksHost::Domain("ok".into()), 80);
        assert_eq!(rep, vec![0x05, 0x00, 0x00, 0x03, 2, b'o', b'k', 0x00, 0x50]);
        let fail = failure_reply(REP_CMD_NOT_SUPPORTED);
        assert_eq!(fail.len(), 10);
        assert_eq!(&fail[..4], &[0x05, 0x07, 0x00, 0x01]);
    }
}
