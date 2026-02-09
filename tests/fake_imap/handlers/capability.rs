//! CAPABILITY command handler.
//!
//! Returns the list of capabilities supported by the fake server.
//! RFC 3501 Section 6.1.1 requires this command.

use crate::fake_imap::io::write_line;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the CAPABILITY command.
pub async fn handle_capability<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    stream: &mut BufReader<S>,
) {
    let _ = write_line(stream, "* CAPABILITY IMAP4rev1 STARTTLS\r\n").await;
    let resp = format!("{tag} OK CAPABILITY completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn run(tag: &str) -> String {
        let (client, server) = tokio::io::duplex(1024);
        let mut stream = BufReader::new(server);

        handle_capability(tag, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn sends_capability_list() {
        let output = run("A1").await;
        assert!(output.contains("* CAPABILITY IMAP4rev1 STARTTLS"));
        assert!(output.contains("A1 OK CAPABILITY completed"));
    }
}
