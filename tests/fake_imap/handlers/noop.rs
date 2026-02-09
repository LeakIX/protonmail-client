//! NOOP command handler.
//!
//! RFC 3501 Section 6.1.2 requires this command. Clients use it for
//! keepalive and polling.

use crate::fake_imap::io::write_line;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the NOOP command.
pub async fn handle_noop<S: AsyncRead + AsyncWrite + Unpin>(tag: &str, stream: &mut BufReader<S>) {
    let resp = format!("{tag} OK NOOP completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn run(tag: &str) -> String {
        let (client, server) = tokio::io::duplex(1024);
        let mut stream = BufReader::new(server);

        handle_noop(tag, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn sends_ok() {
        let output = run("A1").await;
        assert!(output.contains("A1 OK NOOP completed"));
    }
}
