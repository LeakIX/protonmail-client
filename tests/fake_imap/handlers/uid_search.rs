//! UID SEARCH command handler.
//!
//! Matches emails against parsed `SearchKey` criteria from imap-types.
//! We support:
//!
//! - `All` -- returns every UID in the selected folder
//! - `Unseen` / `Seen` -- flag-based filtering
//! - `Since(date)` -- returns UIDs with Date header >= date
//! - `Before(date)` -- returns UIDs with Date header < date
//! - `And`, `Or`, `Not` -- logical combinators
//!
//! The response format (RFC 3501 Section 7.2.5):
//!
//! ```text
//! * SEARCH 1 2 3
//! A0003 OK SEARCH completed
//! ```

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::{Mailbox, TestEmail};
use chrono::NaiveDate;
use imap_codec::imap_types::search::SearchKey;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the UID SEARCH command. Returns matching UIDs from the
/// selected folder.
pub async fn handle_uid_search<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    criteria: &[SearchKey<'_>],
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

    let uids: Vec<u32> = folder
        .emails
        .iter()
        .filter(|e| criteria.iter().all(|key| matches_key(e, key)))
        .map(|e| e.uid)
        .collect();

    // Format: "* SEARCH uid1 uid2 uid3\r\n"
    // If no results, still send "* SEARCH\r\n" (empty result set).
    let uid_str: Vec<String> = uids.iter().map(ToString::to_string).collect();
    let search_line = format!("* SEARCH {}\r\n", uid_str.join(" "));
    let _ = write_line(stream, &search_line).await;
    let resp = format!("{tag} OK SEARCH completed\r\n");
    let _ = write_line(stream, &resp).await;
}

/// Check if a test email matches a single `SearchKey`.
#[allow(clippy::match_same_arms)]
fn matches_key(email: &TestEmail, key: &SearchKey<'_>) -> bool {
    match key {
        SearchKey::All => true,
        SearchKey::Unseen => !email.seen,
        SearchKey::Seen => email.seen,
        SearchKey::Since(date) => parse_email_date(&email.raw).is_some_and(|d| d >= *date.as_ref()),
        SearchKey::Before(date) => parse_email_date(&email.raw).is_some_and(|d| d < *date.as_ref()),
        SearchKey::And(keys) => keys.as_ref().iter().all(|k| matches_key(email, k)),
        SearchKey::Or(a, b) => matches_key(email, a) || matches_key(email, b),
        SearchKey::Not(k) => !matches_key(email, k),
        // Fallback: return all (like current behavior for unknown
        // criteria).
        _ => true,
    }
}

/// Extract the `Date:` header from raw RFC 2822 email bytes and parse
/// it into a `NaiveDate`.
fn parse_email_date(raw: &[u8]) -> Option<NaiveDate> {
    let text = std::str::from_utf8(raw).ok()?;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Date:") {
            let date_str = value.trim();
            return chrono::DateTime::parse_from_rfc2822(date_str)
                .ok()
                .map(|dt| dt.date_naive());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use imap_codec::imap_types::datetime::NaiveDate as ImapDate;
    use tokio::io::BufReader;

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    fn make_dated_email(date: &str) -> Vec<u8> {
        format!(
            "From: a@b.com\r\n\
             Date: {date}\r\n\
             Subject: Test\r\n\
             \r\n\
             Body"
        )
        .into_bytes()
    }

    async fn run(
        tag: &str,
        criteria: &[SearchKey<'_>],
        mailbox: &Mailbox,
        selected: Option<&str>,
    ) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_uid_search(tag, criteria, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> ImapDate {
        ImapDate::unvalidated(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }

    fn chrono_date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
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

        let output = run("A1", &[SearchKey::All], &mailbox, Some("INBOX")).await;

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

        let output = run("A1", &[SearchKey::Unseen], &mailbox, Some("INBOX")).await;

        assert!(output.contains("* SEARCH 2\r\n"));
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", &[SearchKey::All], &mailbox, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }

    #[tokio::test]
    async fn missing_folder_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", &[SearchKey::All], &mailbox, Some("Gone")).await;

        assert!(output.contains("A1 BAD Folder not found"));
    }

    #[tokio::test]
    async fn empty_folder_returns_empty_search() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", &[SearchKey::All], &mailbox, Some("INBOX")).await;

        assert!(output.contains("* SEARCH \r\n"));
        assert!(output.contains("A1 OK SEARCH completed"));
    }

    #[tokio::test]
    async fn since_filters_older_emails() {
        let old = make_dated_email("Mon, 01 Jan 2024 10:00:00 +0000");
        let new = make_dated_email("Mon, 15 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &old)
            .email(2, true, &new)
            .build();

        let output = run(
            "A1",
            &[SearchKey::Since(date(2024, 1, 10))],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        // Only UID 2 (Jan 15) should match; UID 1 (Jan 1) is
        // before Jan 10.
        assert!(output.contains("* SEARCH 2\r\n"));
    }

    #[tokio::test]
    async fn before_filters_newer_emails() {
        let old = make_dated_email("Mon, 01 Jan 2024 10:00:00 +0000");
        let new = make_dated_email("Mon, 15 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &old)
            .email(2, true, &new)
            .build();

        let output = run(
            "A1",
            &[SearchKey::Before(date(2024, 1, 10))],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        // Only UID 1 (Jan 1) should match; UID 2 (Jan 15) is
        // >= Jan 10.
        assert!(output.contains("* SEARCH 1\r\n"));
    }

    #[tokio::test]
    async fn since_before_combined_range() {
        let jan1 = make_dated_email("Mon, 01 Jan 2024 10:00:00 +0000");
        let jan10 = make_dated_email("Wed, 10 Jan 2024 10:00:00 +0000");
        let jan20 = make_dated_email("Sat, 20 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &jan1)
            .email(2, true, &jan10)
            .email(3, true, &jan20)
            .build();

        let output = run(
            "A1",
            &[
                SearchKey::Since(date(2024, 1, 5)),
                SearchKey::Before(date(2024, 1, 15)),
            ],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        // Only UID 2 (Jan 10) falls in [Jan 5, Jan 15).
        assert!(output.contains("* SEARCH 2\r\n"));
    }

    #[tokio::test]
    async fn since_is_inclusive() {
        let exact = make_dated_email("Wed, 10 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &exact)
            .build();

        // SINCE is >= so an email on exactly Jan 10 should match.
        let output = run(
            "A1",
            &[SearchKey::Since(date(2024, 1, 10))],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        assert!(output.contains("* SEARCH 1\r\n"));
    }

    #[tokio::test]
    async fn before_is_exclusive() {
        let exact = make_dated_email("Wed, 10 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &exact)
            .build();

        // BEFORE is < so an email on exactly Jan 10 should NOT match.
        let output = run(
            "A1",
            &[SearchKey::Before(date(2024, 1, 10))],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        assert!(output.contains("* SEARCH \r\n"));
    }

    #[tokio::test]
    async fn emails_without_date_header_excluded() {
        let no_date = make_raw_email(); // no Date: header
        let with_date = make_dated_email("Wed, 10 Jan 2024 10:00:00 +0000");

        let mailbox = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, true, &no_date)
            .email(2, true, &with_date)
            .build();

        let output = run(
            "A1",
            &[SearchKey::Since(date(2024, 1, 1))],
            &mailbox,
            Some("INBOX"),
        )
        .await;

        // Only UID 2 has a parseable date.
        assert!(output.contains("* SEARCH 2\r\n"));
    }

    #[test]
    fn parse_email_date_extracts_date() {
        let raw = make_dated_email("Mon, 01 Jan 2024 12:00:00 +0000");
        let d = parse_email_date(&raw);
        assert_eq!(d, Some(chrono_date(2024, 1, 1)));
    }

    #[test]
    fn parse_email_date_missing_header() {
        let raw = make_raw_email();
        assert!(parse_email_date(&raw).is_none());
    }
}
