//! Test data model for the fake IMAP server
//!
//! Provides a builder-style API for constructing mailbox state:
//!
//! ```ignore
//! let mailbox = MailboxBuilder::new()
//!     .folder("INBOX")
//!         .email(1, false, raw_rfc2822_bytes)
//!         .email(2, true, raw_rfc2822_bytes)
//!     .folder("Sent")
//!         .email(10, true, raw_rfc2822_bytes)
//!     .build();
//! ```
//!
//! The `Mailbox` is shared with the fake IMAP server via `Arc` so the
//! server knows which folders exist, what emails they contain, and
//! whether each email has been read (the `\Seen` flag).

/// A complete mailbox: a collection of named folders, each holding
/// zero or more test emails.
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub folders: Vec<Folder>,
}

impl Mailbox {
    /// Look up a folder by name (case-sensitive, matching real IMAP).
    pub fn get_folder(&self, name: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.name == name)
    }
}

/// A single IMAP folder (e.g. "INBOX", "Sent", "Trash").
#[derive(Debug, Clone)]
pub struct Folder {
    pub name: String,
    pub emails: Vec<TestEmail>,
}

/// A test email stored in a folder.
///
/// - `uid`: IMAP UID -- a unique-per-folder number that never changes
///   (unlike sequence numbers which shift on delete).
/// - `seen`: whether the `\Seen` flag is set. IMAP uses this to track
///   read/unread state. The UNSEEN search returns emails without it.
/// - `raw`: the complete RFC 2822 message (headers + body) as bytes.
///   This is what gets returned in a FETCH BODY[] response.
#[derive(Debug, Clone)]
pub struct TestEmail {
    pub uid: u32,
    pub seen: bool,
    pub raw: Vec<u8>,
}

/// Builder for constructing a `Mailbox` step by step.
///
/// Call `.folder(name)` to start a new folder, then chain
/// `.email(uid, seen, raw)` calls to add messages to it.
/// Finish with `.build()` to get the final `Mailbox`.
pub struct MailboxBuilder {
    folders: Vec<Folder>,
}

impl MailboxBuilder {
    pub fn new() -> Self {
        Self {
            folders: Vec::new(),
        }
    }

    /// Add a new folder. Subsequent `.email()` calls add to this folder.
    pub fn folder(mut self, name: &str) -> Self {
        self.folders.push(Folder {
            name: name.to_string(),
            emails: Vec::new(),
        });
        self
    }

    /// Add an email to the most recently added folder.
    ///
    /// # Panics
    ///
    /// Panics if called before any `.folder()` call.
    pub fn email(mut self, uid: u32, seen: bool, raw: &[u8]) -> Self {
        self.folders
            .last_mut()
            .expect("call .folder() before .email()")
            .emails
            .push(TestEmail {
                uid,
                seen,
                raw: raw.to_vec(),
            });
        self
    }

    /// Consume the builder and return the finished `Mailbox`.
    pub fn build(self) -> Mailbox {
        Mailbox {
            folders: self.folders,
        }
    }
}
