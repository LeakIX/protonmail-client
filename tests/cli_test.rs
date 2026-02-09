#![allow(clippy::similar_names)]

//! End-to-end tests for the `proton-cli` binary.
//!
//! Each test starts a [`FakeImapServer`] on a random port, spawns the
//! compiled `proton-cli` binary as a child process with environment
//! variables pointing at the fake server, and asserts on stdout.

mod fake_imap;

use fake_imap::{FakeImapServer, MailboxBuilder};

/// Build a minimal valid RFC 2822 email.
fn make_raw_email(from: &str, to: &str, subject: &str, body: &str, date: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\n\
         To: {to}\r\n\
         Subject: {subject}\r\n\
         Date: {date}\r\n\
         Message-ID: <test-{subject}@fake.test>\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         {body}"
    )
    .into_bytes()
}

/// Run the `proton-cli` binary with the given arguments, connecting to
/// the provided fake IMAP server. Returns `(stdout, stderr, success)`.
async fn run_cli(server: &FakeImapServer, args: &[&str]) -> (String, String, bool) {
    let bin = env!("CARGO_BIN_EXE_proton-cli");
    let output = tokio::process::Command::new(bin)
        .args(args)
        .env("IMAP_HOST", "127.0.0.1")
        .env("IMAP_PORT", server.port().to_string())
        .env("IMAP_USERNAME", "testuser")
        .env("IMAP_PASSWORD", "testpass")
        .output()
        .await
        .expect("failed to run proton-cli");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, output.status.success())
}

// ── Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_folders() {
    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .folder("Sent")
        .folder("Trash")
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["folders"]).await;

    assert!(success, "proton-cli folders failed");
    assert!(stdout.contains("INBOX"));
    assert!(stdout.contains("Sent"));
    assert!(stdout.contains("Trash"));
}

#[tokio::test]
async fn test_list_limit() {
    let email1 = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "First",
        "First email.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let email2 = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "Second",
        "Second email.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );
    let email3 = make_raw_email(
        "dave@example.com",
        "bob@example.com",
        "Third",
        "Third email.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &email1)
        .email(2, true, &email2)
        .email(3, true, &email3)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["list", "--limit", "2"]).await;

    assert!(success, "proton-cli list --limit failed");

    // Table header should be present.
    assert!(stdout.contains("UID"));
    assert!(stdout.contains("From"));
    assert!(stdout.contains("Subject"));

    // Only 2 of the 3 emails should appear.
    assert!(stdout.contains("2 email(s)"));
}

#[tokio::test]
async fn test_list_unseen() {
    let seen = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Read message",
        "Already read.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let unseen = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "New message",
        "Not yet read.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &seen)
        .email(2, false, &unseen)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["list", "--unseen"]).await;

    assert!(success, "proton-cli list --unseen failed");

    // Only the unseen email should be in the output.
    assert!(stdout.contains("charlie@example.com"));
    assert!(!stdout.contains("alice@example.com"));
    assert!(stdout.contains("1 email(s)"));
}

#[tokio::test]
async fn test_show() {
    let raw = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Hello Bob",
        "This is a test email.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(42, false, &raw)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["show", "42"]).await;

    assert!(success, "proton-cli show failed");
    assert!(stdout.contains("UID:     42"));
    assert!(stdout.contains("alice@example.com"));
    assert!(stdout.contains("bob@example.com"));
    assert!(stdout.contains("Hello Bob"));
    assert!(stdout.contains("This is a test email."));
}

#[tokio::test]
async fn test_search() {
    let email1 = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Important",
        "Urgent matter.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let email2 = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "Casual",
        "Just saying hi.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &email1)
        .email(2, true, &email2)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["search", "ALL"]).await;

    assert!(success, "proton-cli search failed");
    assert!(stdout.contains("alice@example.com"));
    assert!(stdout.contains("charlie@example.com"));
    assert!(stdout.contains("2 email(s)"));
}

#[tokio::test]
async fn test_list_json() {
    let email1 = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "First",
        "First email.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let email2 = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "Second",
        "Second email.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &email1)
        .email(2, true, &email2)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let (stdout, _, success) = run_cli(&server, &["--json", "list", "--limit", "2"]).await;

    assert!(success, "proton-cli --json list failed");

    let emails: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is not valid JSON");

    let arr = emails.as_array().expect("JSON output should be an array");
    assert_eq!(arr.len(), 2);

    // Each entry should have uid, from, and subject fields.
    for entry in arr {
        assert!(entry.get("uid").is_some(), "missing uid field");
        assert!(entry.get("from").is_some(), "missing from field");
        assert!(entry.get("subject").is_some(), "missing subject field");
    }
}
