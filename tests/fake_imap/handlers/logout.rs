//! LOGOUT command handler.
//!
//! The server sends a BYE untagged response (indicating the connection
//! is ending) followed by the tagged OK.

use crate::fake_imap::io::write_line;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the LOGOUT command. Sends BYE + tagged OK.
pub async fn handle_logout<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    stream: &mut BufReader<S>,
) {
    let _ = write_line(stream, "* BYE\r\n").await;
    let resp = format!("{tag} OK LOGOUT completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn run(tag: &str) -> String {
        let (client, server) = tokio::io::duplex(1024);
        let mut stream = BufReader::new(server);

        handle_logout(tag, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn sends_bye_then_ok() {
        let output = run("A0005").await;
        assert!(output.starts_with("* BYE\r\n"));
        assert!(output.contains("A0005 OK LOGOUT completed\r\n"));
    }

    #[tokio::test]
    async fn bye_comes_before_ok() {
        let output = run("X1").await;
        let bye_pos = output.find("* BYE").unwrap();
        let ok_pos = output.find("X1 OK").unwrap();
        assert!(bye_pos < ok_pos);
    }
}
