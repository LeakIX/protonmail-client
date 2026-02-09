//! UID SEARCH command handler.
//!
//! Parses the search criteria and returns matching UIDs. We support:
//!
//! - `ALL` -- returns every UID in the selected folder
//! - `UNSEEN` -- returns UIDs without the `\Seen` flag
//! - `SINCE <date> BEFORE <date>` -- returns all UIDs (date filtering
//!   would require parsing the email Date header, which is overkill
//!   for our tests)
//!
//! The response format (RFC 3501 Section 7.2.5):
//!
//! ```text
//! * SEARCH 1 2 3
//! A0003 OK SEARCH completed
//! ```

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the UID SEARCH command. Returns matching UIDs from the
/// selected folder.
pub async fn handle_uid_search<S: AsyncRead + AsyncWrite + Unpin>(
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

    // Extract the search criteria after "UID SEARCH ".
    let criteria = rest
        .strip_prefix("UID SEARCH ")
        .or_else(|| rest.strip_prefix("uid search "))
        .unwrap_or(rest)
        .to_uppercase();

    let uids: Vec<u32> = if criteria.contains("UNSEEN") {
        // UNSEEN: only emails without the \Seen flag
        folder
            .emails
            .iter()
            .filter(|e| !e.seen)
            .map(|e| e.uid)
            .collect()
    } else {
        // ALL, SINCE/BEFORE, or anything else: return all UIDs.
        folder.emails.iter().map(|e| e.uid).collect()
    };

    // Format: "* SEARCH uid1 uid2 uid3\r\n"
    // If no results, still send "* SEARCH\r\n" (empty result set).
    let uid_str: Vec<String> = uids.iter().map(ToString::to_string).collect();
    let search_line = format!("* SEARCH {}\r\n", uid_str.join(" "));
    let _ = write_line(stream, &search_line).await;
    let resp = format!("{tag} OK SEARCH completed\r\n");
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

        handle_uid_search(tag, rest, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn search_all_returns_all_uids() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &raw)
            .email(2, false, &raw)
            .email(5, true, &raw)
            .build();

        let output = run("A1", "UID SEARCH ALL", &mailbox, Some("INBOX")).await;

        assert!(output.contains("* SEARCH 1 2 5"));
        assert!(output.contains("A1 OK SEARCH completed"));
    }

    #[tokio::test]
    async fn search_unseen_filters_seen() {
        let raw = make_raw_email();
        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &raw) // seen
            .email(2, false, &raw) // unseen
            .email(3, true, &raw) // seen
            .build();

        let output = run("A1", "UID SEARCH UNSEEN", &mailbox, Some("INBOX")).await;

        assert!(output.contains("* SEARCH 2\r\n"));
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", "UID SEARCH ALL", &mailbox, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }

    #[tokio::test]
    async fn missing_folder_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", "UID SEARCH ALL", &mailbox, Some("Gone")).await;

        assert!(output.contains("A1 BAD Folder not found"));
    }

    #[tokio::test]
    async fn empty_folder_returns_empty_search() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", "UID SEARCH ALL", &mailbox, Some("INBOX")).await;

        assert!(output.contains("* SEARCH \r\n"));
        assert!(output.contains("A1 OK SEARCH completed"));
    }
}
