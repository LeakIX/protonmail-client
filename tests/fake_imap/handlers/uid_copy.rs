//! UID COPY command handler.
//!
//! Copies messages from the selected folder to a destination folder.
//! The original messages remain in the source folder.

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use imap_codec::imap_types::sequence::{SeqOrUid, Sequence, SequenceSet};
use std::sync::Mutex;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Extract UIDs from a `SequenceSet` (single values only).
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

/// Handle the UID COPY command. Clones emails into the destination
/// folder.
pub async fn handle_uid_copy<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    sequence_set: &SequenceSet,
    dest_folder: &str,
    mailbox: &Mutex<Mailbox>,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    let uids = extract_uids(sequence_set);

    // Check folders exist (quick lock, no await).
    let (src_exists, dest_exists) = {
        let mb = mailbox.lock().unwrap();
        (
            mb.get_folder(folder_name).is_some(),
            mb.get_folder(dest_folder).is_some(),
        )
    };
    if !src_exists {
        let resp = format!("{tag} BAD Source folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    }
    if !dest_exists {
        let resp = format!(
            "{tag} NO [TRYCREATE] Destination folder \
             not found\r\n"
        );
        let _ = write_line(stream, &resp).await;
        return;
    }

    // Perform copy under lock (no await inside).
    {
        let mut mb = mailbox.lock().unwrap();
        let emails_to_copy: Vec<_> = mb
            .get_folder(folder_name)
            .unwrap()
            .emails
            .iter()
            .filter(|e| uids.contains(&e.uid))
            .cloned()
            .collect();

        let dest = mb.get_folder_mut(dest_folder).unwrap();
        for email in emails_to_copy {
            dest.emails.push(email);
        }
        drop(mb);
    }

    let resp = format!("{tag} OK COPY completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use std::num::NonZeroU32;
    use tokio::io::BufReader;

    fn uid_set(uid: u32) -> SequenceSet {
        SequenceSet(
            vec![Sequence::Single(SeqOrUid::Value(
                NonZeroU32::new(uid).unwrap(),
            ))]
            .try_into()
            .unwrap(),
        )
    }

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    async fn run_copy(
        tag: &str,
        seq: &SequenceSet,
        dest: &str,
        mailbox: &Mutex<Mailbox>,
        selected: Option<&str>,
    ) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_uid_copy(tag, seq, dest, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    #[allow(clippy::significant_drop_tightening)]
    async fn copies_email_to_destination() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .folder("Archive")
                .build(),
        );

        let output = run_copy("A1", &uid_set(1), "Archive", &mb, Some("INBOX")).await;

        assert!(output.contains("A1 OK COPY completed"));

        let locked = mb.lock().unwrap();
        let archive = locked.get_folder("Archive").unwrap();
        assert_eq!(archive.emails.len(), 1);
        assert_eq!(archive.emails[0].uid, 1);
    }

    #[tokio::test]
    #[allow(clippy::significant_drop_tightening)]
    async fn source_email_remains() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .folder("Trash")
                .build(),
        );

        let _output = run_copy("A1", &uid_set(1), "Trash", &mb, Some("INBOX")).await;

        let locked = mb.lock().unwrap();
        let inbox = locked.get_folder("INBOX").unwrap();
        assert_eq!(inbox.emails.len(), 1);
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mb = Mutex::new(MailboxBuilder::new().folder("INBOX").build());

        let output = run_copy("A1", &uid_set(1), "Trash", &mb, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }

    #[tokio::test]
    async fn missing_dest_returns_trycreate() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .build(),
        );

        let output = run_copy("A1", &uid_set(1), "NoSuch", &mb, Some("INBOX")).await;

        assert!(output.contains("TRYCREATE"));
    }
}
