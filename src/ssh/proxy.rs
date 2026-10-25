//! Outbound TCP connections through HTTP CONNECT and SOCKS5 proxies.
//!
//! The SSH transport starts on top of the tunnel this module establishes:
//! [`connect_through_proxy`] returns the raw `TcpStream` to feed into
//! `russh::client::connect_stream`. SOCKS5 framing reuses the constants and
//! greeting parser from [`super::socks5`] (the same wire format the -D
//! dynamic forward serves on the other side).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::{ProxyKind, ResolvedProxy};

use super::socks5;

/// Cap on waiting for the proxy's TCP connection to accept.
const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Cap on the protocol handshake after the TCP connection is up.
const PROXY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on buffered handshake bytes — proxy replies are tiny.
const PROXY_MAX_MESSAGE: usize = 8 * 1024;

#[derive(Debug)]
pub enum ProxyError {
    Io(std::io::Error),
    /// The proxy spoke but refused / misbehaved.
    Protocol(String),
    /// The proxy rejected the supplied credentials.
    AuthFailed,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyError::Io(e) => write!(f, "{}", e),
            ProxyError::Protocol(msg) => write!(f, "{}", msg),
            ProxyError::AuthFailed => write!(f, "proxy authentication failed"),
        }
    }
}

impl std::error::Error for ProxyError {}

impl From<std::io::Error> for ProxyError {
    fn from(e: std::io::Error) -> Self {
        ProxyError::Io(e)
    }
}

/// Connect to `target_host:target_port` through `proxy`, returning the
/// established tunnel. The caller speaks TLS/SSH over the returned stream.
pub async fn connect_through_proxy(
    proxy: &ResolvedProxy,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, ProxyError> {
    let stream = tokio::time::timeout(
        PROXY_CONNECT_TIMEOUT,
        TcpStream::connect((proxy.host.as_str(), proxy.port)),
    )
    .await
    .map_err(|_| ProxyError::Protocol(format!(
        "connecting to proxy {}:{} timed out", proxy.host, proxy.port
    )))??;

    tokio::time::timeout(
        PROXY_HANDSHAKE_TIMEOUT,
        async {
            match proxy.kind {
                ProxyKind::Socks5 => socks5_connect(stream, proxy, target_host, target_port).await,
                ProxyKind::Http => http_connect(stream, proxy, target_host, target_port).await,
            }
        },
    )
    .await
    .map_err(|_| ProxyError::Protocol("proxy handshake timed out".to_string()))?
}

/// HTTP CONNECT tunnel (RFC 7231 §4.3.7). Replies 2xx open the tunnel;
/// 407 means the proxy wants (or rejected) credentials.
async fn http_connect(
    mut stream: TcpStream,
    proxy: &ResolvedProxy,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, ProxyError> {
    let mut request = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n"
    );
    if let Some(username) = &proxy.username {
        use std::fmt::Write as _;
        let credentials = format!("{}:{}", username, proxy.password.as_deref().unwrap_or(""));
        let _ = write!(request, "Proxy-Authorization: Basic {}\r\n", base64_encode(credentials.as_bytes()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;

    let response = read_until_double_crlf(&mut stream).await?;
    let text = String::from_utf8_lossy(&response);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    match status {
        "200" | "201" | "202" | "204" => Ok(stream),
        "407" => Err(ProxyError::AuthFailed),
        other => Err(ProxyError::Protocol(format!(
            "proxy refused CONNECT (HTTP status {other})"
        ))),
    }
}

/// SOCKS5 CONNECT (RFC 1928) with optional user/pass auth (RFC 1929).
/// Domain names are passed through for the proxy to resolve; IP literals
/// use their native address type.
async fn socks5_connect(
    mut stream: TcpStream,
    proxy: &ResolvedProxy,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, ProxyError> {
    // 1. Greeting — offer user/pass when credentials exist, plus no-auth
    //    always (the proxy picks whichever it wants).
    let mut offered = vec![socks5::METHOD_NO_AUTH];
    if proxy.username.is_some() || proxy.password.is_some() {
        offered.push(socks5::METHOD_USER_PASS);
    }
    let mut greeting = vec![socks5::SOCKS_VERSION, offered.len() as u8];
    greeting.extend_from_slice(&offered);
    stream.write_all(&greeting).await?;

    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).await?;
    if reply[0] != socks5::SOCKS_VERSION {
        return Err(ProxyError::Protocol(format!(
            "proxy replied with SOCKS version {}", reply[0]
        )));
    }
    if !offered.contains(&reply[1]) {
        return Err(ProxyError::Protocol(format!(
            "proxy picked auth method {:#04x} which was not offered", reply[1]
        )));
    }

    // 2. Optional user/pass sub-negotiation.
    if reply[1] == socks5::METHOD_USER_PASS {
        let username = proxy.username.as_deref().unwrap_or("");
        let password = proxy.password.as_deref().unwrap_or("");
        // RFC 1929 caps both at 255 bytes.
        if username.len() > 255 || password.len() > 255 {
            return Err(ProxyError::Protocol(
                "proxy username/password too long (max 255 bytes)".to_string(),
            ));
        }
        let mut sub = vec![0x01, username.len() as u8];
        sub.extend_from_slice(username.as_bytes());
        sub.push(password.len() as u8);
        sub.extend_from_slice(password.as_bytes());
        stream.write_all(&sub).await?;

        let mut status = [0u8; 2];
        stream.read_exact(&mut status).await?;
        if status[0] != 0x01 {
            return Err(ProxyError::Protocol(format!(
                "proxy replied to auth negotiation with version {}", status[0]
            )));
        }
        if status[1] != 0x00 {
            return Err(ProxyError::AuthFailed);
        }
    }

    // 3. CONNECT request.
    let mut request = vec![socks5::SOCKS_VERSION, socks5::CMD_CONNECT, 0x00];
    match target_host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => {
            request.push(socks5::ATYP_IPV4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => match target_host.parse::<std::net::Ipv6Addr>() {
            Ok(ip) => {
                request.push(socks5::ATYP_IPV6);
                request.extend_from_slice(&ip.octets());
            }
            Err(_) => {
                if target_host.len() > 255 {
                    return Err(ProxyError::Protocol(
                        "target hostname too long for SOCKS5 (max 255 bytes)".to_string(),
                    ));
                }
                request.push(socks5::ATYP_DOMAIN);
                request.push(target_host.len() as u8);
                request.extend_from_slice(target_host.as_bytes());
            }
        },
    }
    request.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&request).await?;

    // 4. Reply: VER REP RSV ATYP ADDR PORT.
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != socks5::SOCKS_VERSION {
        return Err(ProxyError::Protocol(format!(
            "proxy replied to CONNECT with version {}", header[0]
        )));
    }
    if header[1] != 0x00 {
        return Err(ProxyError::Protocol(format!(
            "proxy CONNECT failed (reply code {:#04x})", header[1]
        )));
    }
    let skip = match header[3] {
        socks5::ATYP_IPV4 => 4 + 2,
        socks5::ATYP_DOMAIN => {
            let mut dlen = [0u8; 1];
            stream.read_exact(&mut dlen).await?;
            dlen[0] as usize + 2
        }
        socks5::ATYP_IPV6 => 16 + 2,
        other => {
            return Err(ProxyError::Protocol(format!(
                "proxy CONNECT reply has unknown address type {other:#04x}"
            )))
        }
    };
    let mut rest = vec![0u8; skip];
    stream.read_exact(&mut rest).await?;

    Ok(stream)
}

/// Read from the stream until the CRLFCRLF header terminator (inclusive).
/// A connect reply without a terminator is a protocol error.
async fn read_until_double_crlf(stream: &mut TcpStream) -> Result<Vec<u8>, ProxyError> {
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(ProxyError::Protocol(
                "proxy closed connection before CONNECT reply".to_string(),
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(buf);
        }
        if buf.len() > PROXY_MAX_MESSAGE {
            return Err(ProxyError::Protocol(
                "proxy CONNECT reply too large".to_string(),
            ));
        }
    }
}

/// RFC 4648 base64 (with padding) for the Basic auth header.
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(TABLE[(n >> 18) as usize & 0x3F] as char);
        out.push(TABLE[(n >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 0x3F] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 0x3F] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProxyKind, ResolvedProxy};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn http_proxy(port: u16, username: Option<&str>, password: Option<&str>) -> ResolvedProxy {
        ResolvedProxy {
            kind: ProxyKind::Http,
            host: "127.0.0.1".into(),
            port,
            username: username.map(String::from),
            password: password.map(String::from),
        }
    }

    fn socks5_proxy(port: u16, username: Option<&str>, password: Option<&str>) -> ResolvedProxy {
        ResolvedProxy {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port,
            username: username.map(String::from),
            password: password.map(String::from),
        }
    }

    async fn with_timeout<F: std::future::Future>(fut: F) -> F::Output {
        tokio::time::timeout(Duration::from_secs(5), fut)
            .await
            .expect("test timed out")
    }

    fn base64_decode(input: &str) -> Vec<u8> {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut acc: u32 = 0;
        let mut bits = 0;
        for c in input.bytes() {
            if c == b'=' {
                break;
            }
            let v = TABLE.iter().position(|&t| t == c).expect("valid base64") as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // Roundtrip through the test decoder.
        assert_eq!(base64_decode(&base64_encode(b"user:pw:with:colons")), b"user:pw:with:colons");
    }

    /// Spawn a fake HTTP CONNECT proxy: demands the given credentials
    /// (when Some), replies with `status`, and pipes the tunnel to the
    /// first TCP connection it accepts afterwards... no — to the address
    /// the client asked for via a real echo server set up by the caller.
    /// Returns the proxy port and a channel carrying the request head.
    async fn spawn_http_proxy(
        status: &'static str,
        require: Option<(&'static str, &'static str)>,
    ) -> (u16, tokio::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 1024];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();

            let ok = match require {
                None => !head.contains("Proxy-Authorization"),
                Some((user, pass)) => head
                    .lines()
                    .find(|l| l.starts_with("Proxy-Authorization: Basic "))
                    .map(|l| {
                        let encoded = l["Proxy-Authorization: Basic ".len()..].trim();
                        String::from_utf8_lossy(&base64_decode(encoded)) == format!("{user}:{pass}")
                    })
                    .unwrap_or(false),
            };
            let _ = tx.send(head).await;
            let reply = if ok { status } else { "407 Proxy Authentication Required" };
            let _ = sock
                .write_all(format!("HTTP/1.1 {reply}\r\nProxy-agent: mock\r\n\r\n").as_bytes())
                .await;
            if ok {
                // Tunnel established: echo whatever the client sends.
                let mut buf = [0u8; 256];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        (port, rx)
    }

    #[tokio::test]
    async fn http_connect_no_auth_tunnels() {
        let (port, mut rx) = spawn_http_proxy("200 Connection established", None).await;
        let proxy = http_proxy(port, None, None);
        let mut stream = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect("CONNECT succeeds");

        let head = with_timeout(rx.recv()).await.unwrap();
        assert!(head.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"), "{head}");
        assert!(!head.contains("Proxy-Authorization"));

        stream.write_all(b"ping").await.unwrap();
        let mut echo = [0u8; 4];
        with_timeout(stream.read_exact(&mut echo)).await.unwrap();
        assert_eq!(&echo, b"ping");
    }

    #[tokio::test]
    async fn http_connect_with_basic_auth() {
        let (port, _rx) =
            spawn_http_proxy("200 Connection established", Some(("user", "s3cret"))).await;
        let proxy = http_proxy(port, Some("user"), Some("s3cret"));
        let mut stream = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect("CONNECT with credentials succeeds");
        stream.write_all(b"pong").await.unwrap();
        let mut echo = [0u8; 4];
        with_timeout(stream.read_exact(&mut echo)).await.unwrap();
        assert_eq!(&echo, b"pong");
    }

    #[tokio::test]
    async fn http_connect_missing_credentials_gets_auth_failed() {
        let (port, _rx) =
            spawn_http_proxy("200 Connection established", Some(("user", "pw"))).await;
        let proxy = http_proxy(port, None, None);
        let err = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect_err("407 must surface as AuthFailed");
        assert!(matches!(err, ProxyError::AuthFailed), "{err}");
    }

    #[tokio::test]
    async fn http_connect_wrong_credentials_gets_auth_failed() {
        let (port, _rx) =
            spawn_http_proxy("200 Connection established", Some(("user", "pw"))).await;
        let proxy = http_proxy(port, Some("user"), Some("wrong"));
        let err = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect_err("rejected credentials must surface as AuthFailed");
        assert!(matches!(err, ProxyError::AuthFailed), "{err}");
    }

    #[tokio::test]
    async fn http_connect_server_error_is_protocol() {
        let (port, _rx) = spawn_http_proxy("502 Bad Gateway", None).await;
        let proxy = http_proxy(port, None, None);
        let err = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect_err("5xx must surface as Protocol");
        assert!(
            matches!(err, ProxyError::Protocol(ref m) if m.contains("502")),
            "{err}"
        );
    }

    /// Spawn a fake SOCKS5 proxy reusing the shared wire constants and the
    /// greeting parser from `socks5` (the same module the -D server uses).
    async fn spawn_socks5_proxy(require_auth: bool) -> u16 {
        use crate::ssh::socks5 as s5;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 256];

            // Greeting.
            loop {
                match s5::parse_greeting(
                    &buf,
                    if require_auth {
                        &[s5::METHOD_USER_PASS]
                    } else {
                        &[s5::METHOD_NO_AUTH]
                    },
                ) {
                    Ok((consumed, method)) => {
                        buf.drain(..consumed);
                        let _ = sock.write_all(&[s5::SOCKS_VERSION, method]).await;
                        break;
                    }
                    Err(s5::SocksError::Incomplete) => {}
                    Err(_) => return,
                }
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
            }

            // Optional user/pass sub-negotiation (accept user/pw only).
            if require_auth {
                // Read until the full sub-negotiation is buffered.
                loop {
                    if buf.len() >= 2 {
                        let ulen = buf[1] as usize;
                        if buf.len() >= 3 + ulen {
                            let plen = buf[2 + ulen] as usize;
                            if buf.len() >= 3 + ulen + plen {
                                break;
                            }
                        }
                    }
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let ulen = buf[1] as usize;
                let plen = buf[2 + ulen] as usize;
                let user = String::from_utf8_lossy(&buf[2..2 + ulen]).to_string();
                let pass = String::from_utf8_lossy(&buf[3 + ulen..3 + ulen + plen]).to_string();
                buf.drain(..3 + ulen + plen);
                if user != "user" || pass != "pw" {
                    let _ = sock.write_all(&[0x01, 0x01]).await;
                    return;
                }
                let _ = sock.write_all(&[0x01, 0x00]).await;
            }

            // CONNECT request: read until a full request is in the buffer.
            loop {
                if buf.len() >= 5 && buf[0] == s5::SOCKS_VERSION && buf[1] == s5::CMD_CONNECT {
                    let need = match buf[3] {
                        s5::ATYP_IPV4 => 10,
                        s5::ATYP_IPV6 => 22,
                        s5::ATYP_DOMAIN => 7 + buf[4] as usize,
                        _ => 0,
                    };
                    if need > 0 && buf.len() >= need {
                        break;
                    }
                }
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let _ = sock.write_all(&s5::success_reply(&s5::SocksHost::Ipv4(
                std::net::Ipv4Addr::LOCALHOST,
            ), 0)).await;

            // Tunnel: echo everything.
            loop {
                match sock.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if sock.write_all(&chunk[..n]).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        port
    }

    #[tokio::test]
    async fn socks5_no_auth_tunnels_ipv4_target() {
        let port = spawn_socks5_proxy(false).await;
        let proxy = socks5_proxy(port, None, None);
        let mut stream = with_timeout(connect_through_proxy(&proxy, "127.0.0.1", 443))
            .await
            .expect("SOCKS5 CONNECT succeeds");
        stream.write_all(b"hello").await.unwrap();
        let mut echo = [0u8; 5];
        with_timeout(stream.read_exact(&mut echo)).await.unwrap();
        assert_eq!(&echo, b"hello");
    }

    #[tokio::test]
    async fn socks5_with_auth_and_domain_target() {
        let port = spawn_socks5_proxy(true).await;
        let proxy = socks5_proxy(port, Some("user"), Some("pw"));
        let mut stream = with_timeout(connect_through_proxy(&proxy, "example.internal", 8443))
            .await
            .expect("SOCKS5 CONNECT with auth succeeds");
        stream.write_all(b"ok").await.unwrap();
        let mut echo = [0u8; 2];
        with_timeout(stream.read_exact(&mut echo)).await.unwrap();
        assert_eq!(&echo, b"ok");
    }

    #[tokio::test]
    async fn socks5_wrong_credentials_get_auth_failed() {
        let port = spawn_socks5_proxy(true).await;
        let proxy = socks5_proxy(port, Some("user"), Some("nope"));
        let err = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect_err("bad sub-negotiation must surface as AuthFailed");
        assert!(matches!(err, ProxyError::AuthFailed), "{err}");
    }

    #[tokio::test]
    async fn socks5_missing_credentials_get_auth_failed() {
        // The mock only accepts user/pass; a client that offers no
        // credentials must not proceed.
        let port = spawn_socks5_proxy(true).await;
        let proxy = socks5_proxy(port, None, None);
        let err = with_timeout(connect_through_proxy(&proxy, "example.com", 443))
            .await
            .expect_err("proxy demands auth, client has none");
        // The mock closes the connection mid-handshake → Io, or the
        // sub-negotiation read gets EOF. Either way it must not succeed.
        assert!(matches!(err, ProxyError::Io(_) | ProxyError::Protocol(_)), "{err}");
    }

    #[tokio::test]
    async fn proxy_connect_timeout_surfaces_as_error() {
        // Port 1 on loopback is reliably refused — an Io error, fast.
        let proxy = socks5_proxy(1, None, None);
        let result = with_timeout(connect_through_proxy(&proxy, "example.com", 443)).await;
        assert!(result.is_err(), "refused connection must error");
    }
}
