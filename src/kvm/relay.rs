//! A local relay for a console's own connection: iDRAC's console talks to its BMC over a WebSocket
//! on another port (`wss://bmc:5900`), with the BMC's self-signed certificate, and a browser view
//! can't be told to trust a certificate for a WebSocket (only for pages: web.rs). The console's page
//! connects here instead (`ws://127.0.0.1:<port>/<token>/<host>/<port>/<path>`), and the relay goes
//! on to the BMC with TLS, trusting only the certificate pinned for that BMC (mod.rs `trust`).

use crate::config::Known;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::OnceCell;

static RELAY: OnceCell<Option<String>> = OnceCell::const_new();

/// The relay's address, `ws://127.0.0.1:<port>/<token>`, started on first use; None if it can't listen.
pub async fn address() -> Option<String> {
    RELAY
        .get_or_init(|| async {
            let listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
            let port = listener.local_addr().ok()?.port();
            // Only the console pages xtermium writes know it: other local programs can't use the relay.
            let token = crate::vault::hex(&crate::vault::random::<16>());
            let t = token.clone();
            tauri::async_runtime::spawn(async move {
                while let Ok((s, _)) = listener.accept().await {
                    let t = t.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = relay(s, &t).await;
                    });
                }
            });
            Some(format!("ws://127.0.0.1:{port}/{token}"))
        })
        .await
        .clone()
}

/// The pinned certificate fingerprints of the BMC at `host` (pins are `host:port` of its web interface).
fn pins(host: &str) -> Vec<String> {
    Known::with(|k| k.https.iter().filter(|(id, _)| id.rsplit_once(':').is_some_and(|(h, _)| h == host)).map(|(_, fp)| fp.clone()).collect())
        .unwrap_or_default()
}

/// One connection: the request's head names the BMC and port; it goes there with its own path and
/// Host, then everything passes through as it is.
async fn relay(mut page: TcpStream, token: &str) -> anyhow::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = page.read(&mut buf).await?;
        anyhow::ensure!(n > 0 && head.len() < 16_384, "no request");
        head.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let (head_text, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let (request, headers) = head_text.split_once("\r\n").unwrap_or((head_text, ""));
    let (host, port, request) = target(request, token).ok_or_else(|| anyhow::anyhow!("not a console connection"))?;
    let pinned = pins(&host);
    anyhow::ensure!(!pinned.is_empty(), "{host} has no trusted certificate");
    let headers: Vec<String> = headers
        .split("\r\n")
        .map(|h| if h.to_ascii_lowercase().starts_with("host:") { format!("Host: {host}:{port}") } else { h.to_string() })
        .collect();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { pinned, provider }))
        .with_no_client_auth();
    let bmc = TcpStream::connect((host.as_str(), port)).await?;
    let mut bmc = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(ServerName::try_from(host.clone())?, bmc).await?;
    bmc.write_all(format!("{request}\r\n{}\r\n\r\n{body}", headers.join("\r\n")).as_bytes()).await?;
    tokio::io::copy_bidirectional(&mut page, &mut bmc).await?;
    Ok(())
}

/// `GET /<token>/<host>/<port>/<path> HTTP/1.1` → the BMC, its port, and the request line for it.
fn target(request: &str, token: &str) -> Option<(String, u16, String)> {
    let mut parts = request.splitn(3, ' ');
    let (method, path, version) = (parts.next()?, parts.next()?, parts.next()?);
    let rest = path.strip_prefix('/')?.strip_prefix(token)?.strip_prefix('/')?;
    let (host, rest) = rest.split_once('/')?;
    let (port, rest) = rest.split_once('/').unwrap_or((rest, ""));
    Some((host.to_string(), port.parse().ok()?, format!("{method} /{rest} {version}")))
}

/// Trusts exactly the pinned certificates (hex SHA-256 of the DER, as the pins keep them).
#[derive(Debug)]
struct Pinned {
    pinned: Vec<String>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = crate::vault::hex(&Sha256::digest(cert.as_ref()));
        if self.pinned.contains(&fp) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("not the BMC's trusted certificate".into()))
        }
    }

    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_their_bmc_and_need_the_token() {
        assert_eq!(
            target("GET /tok/192.0.2.15/5900/ws?a=1 HTTP/1.1", "tok"),
            Some(("192.0.2.15".into(), 5900, "GET /ws?a=1 HTTP/1.1".into()))
        );
        assert_eq!(target("GET /tok/bmc/5900 HTTP/1.1", "tok"), Some(("bmc".into(), 5900, "GET / HTTP/1.1".into())));
        assert_eq!(target("GET /other/bmc/5900/ HTTP/1.1", "tok"), None);
    }
}
