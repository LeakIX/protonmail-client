//! LOGIN command handler.
//!
//! In real IMAP, credentials are sent in plaintext over the (now
//! encrypted) connection. We accept any credentials since this is a
//! test server.

use crate::fake_imap::io::write_line;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the LOGIN command. Accepts any credentials.
pub async fn handle_login<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    stream: &mut BufReader<S>,
) -> bool {
    let resp = format!("{tag} OK LOGIN completed\r\n");
    write_line(stream, &resp).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    /// Create a `BufReader` over an in-memory duplex stream, run the
    /// handler, and return what was written to the client.
    async fn run(tag: &str) -> (String, bool) {
        let (client, server) = tokio::io::duplex(1024);
        let mut stream = BufReader::new(server);

        let ok = handle_login(tag, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        (String::from_utf8(buf).unwrap(), ok)
    }

    #[tokio::test]
    async fn responds_with_ok() {
        let (output, ok) = run("A0001").await;
        assert!(ok);
        assert_eq!(output, "A0001 OK LOGIN completed\r\n");
    }

    #[tokio::test]
    async fn echoes_client_tag() {
        let (output, _) = run("TAG42").await;
        assert!(output.starts_with("TAG42 "));
    }
}
