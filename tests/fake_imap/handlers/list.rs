//! LIST command handler.
//!
//! Responds with one `* LIST` line per folder, followed by the tagged
//! OK. The format follows RFC 3501 Section 7.2.2:
//!
//! ```text
//! * LIST (\HasNoChildren) "/" "INBOX"
//! * LIST (\HasNoChildren) "/" "Sent"
//! A0002 OK LIST completed
//! ```

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the LIST command. Emits one `* LIST` line per folder.
pub async fn handle_list<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    mailbox: &Mailbox,
    stream: &mut BufReader<S>,
) {
    for folder in &mailbox.folders {
        let line = format!("* LIST (\\HasNoChildren) \"/\" \"{}\"\r\n", folder.name);
        if write_line(stream, &line).await.is_err() {
            return;
        }
    }
    let resp = format!("{tag} OK LIST completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use tokio::io::BufReader;

    async fn run(tag: &str, mailbox: &Mailbox) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_list(tag, mailbox, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn lists_all_folders() {
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .folder("Sent")
            .folder("Trash")
            .build();

        let output = run("A1", &mailbox).await;

        assert!(output.contains("\"INBOX\""));
        assert!(output.contains("\"Sent\""));
        assert!(output.contains("\"Trash\""));
    }

    #[tokio::test]
    async fn ends_with_tagged_ok() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let output = run("T1", &mailbox).await;

        assert!(output.ends_with("T1 OK LIST completed\r\n"));
    }

    #[tokio::test]
    async fn empty_mailbox_returns_only_ok() {
        let mailbox = MailboxBuilder::new().build();
        let output = run("T2", &mailbox).await;

        assert_eq!(output, "T2 OK LIST completed\r\n");
    }

    #[tokio::test]
    async fn includes_has_no_children_flag() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let output = run("T3", &mailbox).await;

        assert!(output.contains("\\HasNoChildren"));
    }
}
