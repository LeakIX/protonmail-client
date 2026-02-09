//! SELECT command handler.
//!
//! Opens a folder and responds with metadata. The key pieces are:
//!
//! - `* N EXISTS` -- total number of messages in the folder.
//! - `* OK [UIDVALIDITY V]` -- a value that changes if the folder's
//!   UID space is reset (e.g. the folder was deleted and recreated).
//!   Clients use this to invalidate their UID caches.
//!
//! Returns the selected folder name (or `None` if not found).

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the SELECT command. Returns the selected folder name.
pub async fn handle_select<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    folder_name: &str,
    mailbox: &Mailbox,
    stream: &mut BufReader<S>,
) -> Option<String> {
    if let Some(folder) = mailbox.get_folder(folder_name) {
        // RFC 3501 Section 6.3.1: required FLAGS response
        let _ = write_line(
            stream,
            "* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)\r\n",
        )
        .await;

        let exists = format!("* {} EXISTS\r\n", folder.emails.len());
        let _ = write_line(stream, &exists).await;

        // RFC 3501 Section 6.3.1: required RECENT response
        let _ = write_line(stream, "* 0 RECENT\r\n").await;

        let _ = write_line(stream, "* OK [UIDVALIDITY 1]\r\n").await;

        // RFC 3501 Section 7.1: UIDNEXT
        let uidnext = folder
            .emails
            .iter()
            .map(|e| e.uid)
            .max()
            .map_or(1, |max| max + 1);
        let _ = write_line(stream, &format!("* OK [UIDNEXT {uidnext}]\r\n")).await;

        // RFC 3501 Section 7.1: PERMANENTFLAGS
        let _ = write_line(
            stream,
            "* OK [PERMANENTFLAGS (\\Seen \\Deleted)] Limited\r\n",
        )
        .await;

        // RFC 3501 Section 7.1: UNSEEN (first unseen message)
        if let Some(pos) = folder.emails.iter().position(|e| !e.seen) {
            let _ = write_line(stream, &format!("* OK [UNSEEN {}]\r\n", pos + 1)).await;
        }

        let resp = format!("{tag} OK [READ-WRITE] SELECT completed\r\n");
        let _ = write_line(stream, &resp).await;
        Some(folder_name.to_string())
    } else {
        let resp = format!("{tag} NO Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use tokio::io::BufReader;

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    async fn run(tag: &str, folder_name: &str, mailbox: &Mailbox) -> (String, Option<String>) {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        let selected = handle_select(tag, folder_name, mailbox, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        (String::from_utf8(buf).unwrap(), selected)
    }

    #[tokio::test]
    async fn selects_existing_folder() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, false, &raw)
            .email(2, true, &raw)
            .build();

        let (output, selected) = run("A1", "INBOX", &mailbox).await;

        assert_eq!(selected, Some("INBOX".to_string()));
        assert!(output.contains("* 2 EXISTS"));
        assert!(output.contains("UIDVALIDITY"));
        assert!(output.contains("A1 OK"));
    }

    #[tokio::test]
    async fn returns_none_for_missing_folder() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let (output, selected) = run("A1", "NoSuchFolder", &mailbox).await;

        assert!(selected.is_none());
        assert!(output.contains("A1 NO Folder not found"));
    }

    #[tokio::test]
    async fn exists_count_matches_email_count() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &raw)
            .email(2, true, &raw)
            .email(3, false, &raw)
            .build();

        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* 3 EXISTS"));
    }

    #[tokio::test]
    async fn sends_flags_response() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)"));
    }

    #[tokio::test]
    async fn sends_recent_response() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* 0 RECENT"));
    }

    #[tokio::test]
    async fn sends_uidnext_response() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(5, true, &raw)
            .email(10, true, &raw)
            .build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* OK [UIDNEXT 11]"));
    }

    #[tokio::test]
    async fn sends_uidnext_1_for_empty_folder() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* OK [UIDNEXT 1]"));
    }

    #[tokio::test]
    async fn sends_permanentflags_response() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(output.contains("* OK [PERMANENTFLAGS (\\Seen \\Deleted)] Limited"));
    }

    #[tokio::test]
    async fn sends_unseen_for_first_unseen_message() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &raw)
            .email(2, true, &raw)
            .email(3, false, &raw)
            .build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        // 3rd message (index 2, 1-based = 3) is first unseen
        assert!(output.contains("* OK [UNSEEN 3]"));
    }

    #[tokio::test]
    async fn no_unseen_when_all_read() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &raw)
            .email(2, true, &raw)
            .build();
        let (output, _) = run("A1", "INBOX", &mailbox).await;
        assert!(!output.contains("UNSEEN"));
    }
}
