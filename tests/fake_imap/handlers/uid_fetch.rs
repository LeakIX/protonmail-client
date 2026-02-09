//! UID FETCH command handler.
//!
//! This is the most complex IMAP response because it uses **counted
//! literals** to transfer message bodies. The format is:
//!
//! ```text
//! * <seq> FETCH (UID <uid> BODY[] {<length>}
//! <exactly length bytes of raw RFC 2822 message>
//! )
//! ```
//!
//! The `{length}\r\n` is an IMAP literal marker. It tells the client:
//! "the next `length` bytes are raw data, not IMAP protocol text."
//! After reading those bytes, the client expects the closing `)`.
//!
//! We use the sequence number equal to the UID for simplicity (in real
//! IMAP, sequence numbers are assigned per-session based on the order
//! messages appear in the folder).

use crate::fake_imap::io::{write_bytes, write_line};
use crate::fake_imap::mailbox::Mailbox;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the UID FETCH command. Returns the email body as an IMAP
/// literal.
pub async fn handle_uid_fetch<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    rest: &str,
    mailbox: &Mailbox,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    let Some(folder) = mailbox.get_folder(folder_name) else {
        let resp = format!("{tag} BAD Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    // Parse the UID from "UID FETCH <uid> (BODY.PEEK[])".
    // The format from async-imap is: "UID FETCH 42 (BODY.PEEK[])"
    let parts: Vec<&str> = rest.split_whitespace().collect();
    // parts: ["UID", "FETCH", "42", "(BODY.PEEK[])"]
    let uid: u32 = if parts.len() >= 3 {
        parts[2].parse().unwrap_or(0)
    } else {
        0
    };

    if let Some(email) = folder.emails.iter().find(|e| e.uid == uid) {
        let body_len = email.raw.len();

        // Build the FETCH response with an IMAP literal.
        let header = format!("* {uid} FETCH (UID {uid} BODY[] {{{body_len}}}\r\n");
        if write_line(stream, &header).await.is_err() {
            return;
        }

        // Write the raw email bytes (the literal data).
        if write_bytes(stream, &email.raw).await.is_err() {
            return;
        }

        // Close the FETCH response parenthesis.
        if write_line(stream, ")\r\n").await.is_err() {
            return;
        }
    }

    let resp = format!("{tag} OK FETCH completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use tokio::io::BufReader;

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    async fn run(tag: &str, rest: &str, mailbox: &Mailbox, selected: Option<&str>) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_uid_fetch(tag, rest, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn fetches_email_by_uid() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(42, false, &raw)
            .build();

        let output = run("A1", "UID FETCH 42 (BODY.PEEK[])", &mailbox, Some("INBOX")).await;

        assert!(output.contains("* 42 FETCH (UID 42 BODY[]"));
        assert!(output.contains("From: a@b.com"));
        assert!(output.contains("A1 OK FETCH completed"));
    }

    #[tokio::test]
    async fn literal_length_matches_body() {
        let raw = make_raw_email();
        let expected_len = raw.len();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, false, &raw)
            .build();

        let output = run("A1", "UID FETCH 1 (BODY.PEEK[])", &mailbox, Some("INBOX")).await;

        let literal = format!("{{{expected_len}}}");
        assert!(output.contains(&literal));
    }

    #[tokio::test]
    async fn missing_uid_returns_only_ok() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", "UID FETCH 99 (BODY.PEEK[])", &mailbox, Some("INBOX")).await;

        assert!(!output.contains("FETCH (UID"));
        assert!(output.contains("A1 OK FETCH completed"));
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", "UID FETCH 1 (BODY.PEEK[])", &mailbox, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }
}
