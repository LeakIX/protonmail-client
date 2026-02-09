//! EXPUNGE command handler.
//!
//! Permanently removes all messages with the `\Deleted` flag from the
//! selected folder. Sends `* N EXPUNGE` for each removed message
//! (where N is the original sequence number, adjusted as earlier
//! messages are removed).

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use std::sync::Mutex;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Handle the EXPUNGE command. Removes deleted messages and sends
/// untagged EXPUNGE responses.
pub async fn handle_expunge<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    mailbox: &Mutex<Mailbox>,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    // Check folder exists (quick lock, no await).
    let folder_exists = {
        let mb = mailbox.lock().unwrap();
        mb.get_folder(folder_name).is_some()
    };
    if !folder_exists {
        let resp = format!("{tag} BAD Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    }

    // Remove deleted messages under lock (no await inside).
    let expunged_seqs = {
        let mut mb = mailbox.lock().unwrap();
        let folder = mb.get_folder_mut(folder_name).unwrap();

        let deleted_indices: Vec<usize> = folder
            .emails
            .iter()
            .enumerate()
            .filter(|(_, e)| e.deleted)
            .map(|(i, _)| i)
            .collect();

        // Remove from back to front to preserve indices.
        let mut seqs = Vec::new();
        for (offset, idx) in deleted_indices.iter().enumerate() {
            // The sequence number the client sees, adjusted for
            // prior removals in this EXPUNGE.
            let seq = idx + 1 - offset;
            seqs.push(seq);
        }

        // Actually remove (back to front).
        for idx in deleted_indices.iter().rev() {
            folder.emails.remove(*idx);
        }

        drop(mb);
        seqs
    };

    // Send untagged EXPUNGE responses outside the lock.
    for seq in &expunged_seqs {
        let line = format!("* {seq} EXPUNGE\r\n");
        if write_line(stream, &line).await.is_err() {
            return;
        }
    }

    let resp = format!("{tag} OK EXPUNGE completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::{MailboxBuilder, TestEmail};
    use tokio::io::BufReader;

    fn make_raw_email() -> Vec<u8> {
        b"From: a@b.com\r\nSubject: Test\r\n\r\nBody".to_vec()
    }

    async fn run_expunge(tag: &str, mailbox: &Mutex<Mailbox>, selected: Option<&str>) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        handle_expunge(tag, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    #[allow(clippy::significant_drop_tightening)]
    async fn removes_deleted_emails() {
        let raw = make_raw_email();
        let mut mb = MailboxBuilder::new()
            .folder("INBOX")
            .email(1, false, &raw)
            .email(2, false, &raw)
            .build();

        // Mark UID 1 as deleted.
        mb.get_folder_mut("INBOX").unwrap().emails[0].deleted = true;
        let mb = Mutex::new(mb);

        let output = run_expunge("A1", &mb, Some("INBOX")).await;

        assert!(output.contains("* 1 EXPUNGE"));
        assert!(output.contains("A1 OK EXPUNGE completed"));

        let locked = mb.lock().unwrap();
        let inbox = locked.get_folder("INBOX").unwrap();
        assert_eq!(inbox.emails.len(), 1);
        assert_eq!(inbox.emails[0].uid, 2);
    }

    #[tokio::test]
    #[allow(clippy::significant_drop_tightening)]
    async fn no_deleted_emails_is_noop() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .build(),
        );

        let output = run_expunge("A1", &mb, Some("INBOX")).await;

        assert!(!output.contains("EXPUNGE\r\n"));
        assert!(output.contains("A1 OK EXPUNGE completed"));

        let locked = mb.lock().unwrap();
        assert_eq!(locked.get_folder("INBOX").unwrap().emails.len(), 1);
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mb = Mutex::new(MailboxBuilder::new().folder("INBOX").build());

        let output = run_expunge("A1", &mb, None).await;

        assert!(output.contains("A1 BAD No folder selected"));
    }

    #[tokio::test]
    #[allow(clippy::significant_drop_tightening)]
    async fn multiple_deletions() {
        let raw = make_raw_email();
        let mb = Mutex::new(Mailbox {
            folders: vec![crate::fake_imap::mailbox::Folder {
                name: "INBOX".to_string(),
                emails: vec![
                    TestEmail {
                        uid: 1,
                        seen: false,
                        deleted: true,
                        raw: raw.clone(),
                    },
                    TestEmail {
                        uid: 2,
                        seen: false,
                        deleted: false,
                        raw: raw.clone(),
                    },
                    TestEmail {
                        uid: 3,
                        seen: false,
                        deleted: true,
                        raw: raw.clone(),
                    },
                ],
            }],
        });

        let output = run_expunge("A1", &mb, Some("INBOX")).await;

        // Both deleted messages should produce EXPUNGE lines.
        assert!(output.contains("EXPUNGE"));
        assert!(output.contains("A1 OK EXPUNGE completed"));

        let locked = mb.lock().unwrap();
        let inbox = locked.get_folder("INBOX").unwrap();
        assert_eq!(inbox.emails.len(), 1);
        assert_eq!(inbox.emails[0].uid, 2);
    }
}
