//! UID STORE command handler.
//!
//! Modifies flags on messages identified by UID. Supports:
//!
//! - `+FLAGS (...)` -- add flags
//! - `-FLAGS (...)` -- remove flags
//! - `FLAGS (...)` -- replace flags
//!
//! Responds with `* N FETCH (FLAGS (...))` per modified message,
//! then the tagged OK.

use crate::fake_imap::io::write_line;
use crate::fake_imap::mailbox::Mailbox;
use imap_codec::imap_types::flag::{Flag, StoreResponse, StoreType};
use imap_codec::imap_types::sequence::{SeqOrUid, Sequence, SequenceSet};
use std::sync::Mutex;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

/// Extract UIDs from a `SequenceSet`.
///
/// Supports single values and ranges (e.g. `1,3,5` or `1:*`).
fn extract_uids(seq_set: &SequenceSet, max_uid: u32) -> Vec<u32> {
    let mut uids = Vec::new();
    for seq in seq_set.0.as_ref() {
        match seq {
            Sequence::Single(SeqOrUid::Value(v)) => {
                uids.push(v.get());
            }
            Sequence::Range(a, b) => {
                let lo = match a {
                    SeqOrUid::Value(v) => v.get(),
                    SeqOrUid::Asterisk => max_uid,
                };
                let hi = match b {
                    SeqOrUid::Value(v) => v.get(),
                    SeqOrUid::Asterisk => max_uid,
                };
                let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
                for uid in lo..=hi {
                    uids.push(uid);
                }
            }
            Sequence::Single(_) => {}
        }
    }
    uids
}

/// Parsed STORE command arguments.
pub struct StoreArgs<'a> {
    pub sequence_set: &'a SequenceSet,
    pub kind: &'a StoreType,
    pub response: &'a StoreResponse,
    pub flags: &'a [Flag<'a>],
}

/// Handle the UID STORE command. Modifies flags on matching emails.
pub async fn handle_uid_store<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    args: &StoreArgs<'_>,
    mailbox: &Mutex<Mailbox>,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    // Determine which flags the client wants to set/unset.
    let wants_seen = args.flags.iter().any(|f| matches!(f, Flag::Seen));
    let wants_deleted = args.flags.iter().any(|f| matches!(f, Flag::Deleted));

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

    // Mutate flags under lock (no await inside).
    let results = {
        let mut mb = mailbox.lock().unwrap();
        let folder = mb.get_folder_mut(folder_name).unwrap();

        let max_uid = folder.emails.iter().map(|e| e.uid).max().unwrap_or(0);
        let uids = extract_uids(args.sequence_set, max_uid);

        let mut results: Vec<(usize, u32, Vec<String>)> = Vec::new();

        for uid in uids {
            if let Some((idx, email)) = folder
                .emails
                .iter_mut()
                .enumerate()
                .find(|(_, e)| e.uid == uid)
            {
                match args.kind {
                    StoreType::Add => {
                        if wants_seen {
                            email.seen = true;
                        }
                        if wants_deleted {
                            email.deleted = true;
                        }
                    }
                    StoreType::Remove => {
                        if wants_seen {
                            email.seen = false;
                        }
                        if wants_deleted {
                            email.deleted = false;
                        }
                    }
                    StoreType::Replace => {
                        email.seen = wants_seen;
                        email.deleted = wants_deleted;
                    }
                }

                let mut current = Vec::new();
                if email.seen {
                    current.push("\\Seen".to_string());
                }
                if email.deleted {
                    current.push("\\Deleted".to_string());
                }

                let seq = idx + 1;
                results.push((seq, uid, current));
            }
        }
        drop(mb);
        results
    };

    // Send FETCH responses outside the lock.
    if !matches!(args.response, StoreResponse::Silent) {
        for (seq, uid, flags_list) in &results {
            let flags_str = flags_list.join(" ");
            let line = format!(
                "* {seq} FETCH (UID {uid} \
                 FLAGS ({flags_str}))\r\n"
            );
            if write_line(stream, &line).await.is_err() {
                return;
            }
        }
    }

    let resp = format!("{tag} OK STORE completed\r\n");
    let _ = write_line(stream, &resp).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_imap::mailbox::MailboxBuilder;
    use imap_codec::imap_types::sequence::SequenceSet;
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

    async fn run_store(
        tag: &str,
        seq: &SequenceSet,
        kind: &StoreType,
        response: &StoreResponse,
        flags: &[Flag<'_>],
        mailbox: &Mutex<Mailbox>,
        selected: Option<&str>,
    ) -> String {
        let (client, server) = tokio::io::duplex(4096);
        let mut stream = BufReader::new(server);

        let args = StoreArgs {
            sequence_set: seq,
            kind,
            response,
            flags,
        };
        handle_uid_store(tag, &args, mailbox, selected, &mut stream).await;
        drop(stream);

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut BufReader::new(client), &mut buf)
            .await
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]

    async fn add_seen_flag() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .build(),
        );

        let output = run_store(
            "A1",
            &uid_set(1),
            &StoreType::Add,
            &StoreResponse::Answer,
            &[Flag::Seen],
            &mb,
            Some("INBOX"),
        )
        .await;

        assert!(output.contains("FLAGS (\\Seen)"));
        assert!(output.contains("A1 OK STORE completed"));

        // Verify mutation persisted.
        assert!(mb.lock().unwrap().get_folder("INBOX").unwrap().emails[0].seen);
    }

    #[tokio::test]
    async fn remove_seen_flag() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, true, &raw) // starts seen
                .build(),
        );

        let _output = run_store(
            "A1",
            &uid_set(1),
            &StoreType::Remove,
            &StoreResponse::Answer,
            &[Flag::Seen],
            &mb,
            Some("INBOX"),
        )
        .await;

        assert!(!mb.lock().unwrap().get_folder("INBOX").unwrap().emails[0].seen);
    }

    #[tokio::test]
    async fn add_deleted_flag() {
        let raw = make_raw_email();
        let mb = Mutex::new(
            MailboxBuilder::new()
                .folder("INBOX")
                .email(1, false, &raw)
                .build(),
        );

        let _output = run_store(
            "A1",
            &uid_set(1),
            &StoreType::Add,
            &StoreResponse::Answer,
            &[Flag::Deleted],
            &mb,
            Some("INBOX"),
        )
        .await;

        assert!(mb.lock().unwrap().get_folder("INBOX").unwrap().emails[0].deleted);
    }

    #[tokio::test]
    async fn no_folder_selected_returns_bad() {
        let mb = Mutex::new(MailboxBuilder::new().folder("INBOX").build());

        let output = run_store(
            "A1",
            &uid_set(1),
            &StoreType::Add,
            &StoreResponse::Answer,
            &[Flag::Seen],
            &mb,
            None,
        )
        .await;

        assert!(output.contains("A1 BAD No folder selected"));
    }
}
