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
//! The sequence number is the 1-based index of the message within the
//! folder, per RFC 3501 Section 7.4.2.

use crate::fake_imap::io::{write_bytes, write_line};
use crate::fake_imap::mailbox::Mailbox;
use imap_codec::imap_types::sequence::{SeqOrUid, Sequence, SequenceSet};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Extract UIDs from a `SequenceSet`. We only support single values
/// (not ranges) since that's what `async-imap` sends for individual
/// fetches.
fn extract_uids(seq_set: &SequenceSet) -> Vec<u32> {
    seq_set
        .0
        .as_ref()
        .iter()
        .filter_map(|seq| match seq {
            Sequence::Single(SeqOrUid::Value(v)) => Some(v.get()),
            _ => None,
        })
        .collect()
}

/// Handle the UID FETCH command. Returns the email body as an IMAP
/// literal.
pub async fn handle_uid_fetch<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    sequence_set: &SequenceSet,
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

    let uids = extract_uids(sequence_set);

    for uid in uids {
        if let Some((idx, email)) = folder.emails.iter().enumerate().find(|(_, e)| e.uid == uid) {
            let seq = idx + 1; // 1-based sequence number
            let body_len = email.raw.len();

            let header = format!(
                "* {seq} FETCH (UID {uid} BODY[] \
                 {{{body_len}}}\r\n"
            );
            if write_line(stream, &header).await.is_err() {
                return;
            }

            if write_bytes(stream, &email.raw).await.is_err() {
                return;
            }

            if write_line(stream, ")\r\n").await.is_err() {
                return;
            }
        }
    }

    let resp = format!("{tag} OK FETCH completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use std::num::NonZeroU32;
    use tokio::io::BufReader;

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    fn uid_set(uid: u32) -> SequenceSet {
        SequenceSet(
            vec![Sequence::Single(SeqOrUid::Value(
                NonZeroU32::new(uid).unwrap(),
            ))]
            .try_into()
            .unwrap(),
        )
    }

    async fn run(
        tag: &str,
        sequence_set: &SequenceSet,
        mailbox: &Mailbox,
        selected: Option<&str>,
    ) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_uid_fetch(tag, sequence_set, mailbox, selected, &mut stream).await;
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

        let output = run("A1", &uid_set(42), &mailbox, Some("INBOX")).await;

        // Sequence number is 1 (1st message), UID is 42
        assert!(output.contains("* 1 FETCH (UID 42 BODY[]"));
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

        let output = run("A1", &uid_set(1), &mailbox, Some("INBOX")).await;

        let literal = format!("{{{expected_len}}}");
        assert!(output.contains(&literal));
    }

    #[tokio::test]
    async fn missing_uid_returns_only_ok() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", &uid_set(99), &mailbox, Some("INBOX")).await;

        assert!(!output.contains("FETCH (UID"));
        assert!(output.contains("A1 OK FETCH completed"));
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mailbox = MailboxBuilder::new().folder("INBOX").build();

        let output = run("A1", &uid_set(1), &mailbox, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }
}
