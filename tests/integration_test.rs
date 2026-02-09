#![allow(clippy::similar_names)]

//! Integration tests for `ProtonClient` using the fake IMAP server.
//!
//! Each test constructs a `Mailbox` with test data, starts a
//! `FakeImapServer` on a random port, creates a `ProtonClient`
//! pointing at it, and exercises one of the client's public methods.

mod fake_imap;

use fake_imap::{FakeImapServer, MailboxBuilder};
use protonmail_client::{Flag, Folder, ImapConfig, ProtonClient, ReadWrite};

/// Build a minimal valid RFC 2822 email.
///
/// The format follows RFC 2822: headers separated by CRLF, a blank
/// line (CRLF CRLF) separating headers from body, and the body text.
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

/// Create a read-only `ProtonClient` pointed at the fake server.
fn client_for(server: &FakeImapServer) -> ProtonClient {
    ProtonClient::new(config_for(server))
}

/// Create a read-write `ProtonClient` pointed at the fake server.
fn writer_for(server: &FakeImapServer) -> ProtonClient<ReadWrite> {
    ProtonClient::new(config_for(server))
}

fn config_for(server: &FakeImapServer) -> ImapConfig {
    ImapConfig {
        host: "127.0.0.1".to_string(),
        port: server.port(),
        username: "testuser".to_string(),
        password: "testpass".to_string(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_folders() {
    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .folder("Sent")
        .folder("Trash")
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    let folders = client.list_folders().await.unwrap();
    assert_eq!(folders, vec!["INBOX", "Sent", "Trash"]);
}

#[tokio::test]
async fn test_fetch_uid() {
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
    let client = client_for(&server);

    let email = client.fetch_uid(&Folder::Inbox, 42).await.unwrap();
    assert_eq!(email.uid, 42);
    assert_eq!(email.from.address, "alice@example.com");
    assert_eq!(email.subject.original, "Hello Bob");
}

#[tokio::test]
async fn test_fetch_unseen() {
    let seen_email = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Read message",
        "Already read.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let unseen_email = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "New message",
        "Not yet read.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &seen_email) // seen = true -> \Seen flag
        .email(2, false, &unseen_email) // seen = false -> UNSEEN
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    let emails = client.fetch_unseen(&Folder::Inbox).await.unwrap();

    // Only the unseen email should be returned.
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].uid, 2);
    assert_eq!(emails[0].from.address, "charlie@example.com");
}

#[tokio::test]
async fn test_fetch_all() {
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
        .email(2, false, &email2)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    let emails = client.fetch_all(&Folder::Inbox).await.unwrap();
    assert_eq!(emails.len(), 2);
}

#[tokio::test]
async fn test_fetch_last_n() {
    let email1 = make_raw_email(
        "a@example.com",
        "b@example.com",
        "Oldest",
        "Oldest email.",
        "Mon, 01 Jan 2024 08:00:00 +0000",
    );
    let email2 = make_raw_email(
        "c@example.com",
        "b@example.com",
        "Middle",
        "Middle email.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let email3 = make_raw_email(
        "d@example.com",
        "b@example.com",
        "Newest",
        "Newest email.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &email1)
        .email(2, true, &email2)
        .email(3, true, &email3)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    // Request the 2 most recent emails.
    let emails = client.fetch_last_n(&Folder::Inbox, 2).await.unwrap();
    assert_eq!(emails.len(), 2);

    // fetch_last_n sorts descending by date, so newest first.
    assert_eq!(emails[0].from.address, "d@example.com");
    assert_eq!(emails[1].from.address, "c@example.com");
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
    let client = client_for(&server);

    // Our fake server returns all UIDs for any non-UNSEEN query,
    // so an "ALL" search should return both.
    let emails = client.search(&Folder::Inbox, "ALL").await.unwrap();
    assert_eq!(emails.len(), 2);
}

#[tokio::test]
async fn test_fetch_date_range() {
    let jan1 = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "New Year",
        "Happy new year!",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let jan10 = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "Mid January",
        "Midway through the month.",
        "Wed, 10 Jan 2024 10:00:00 +0000",
    );
    let jan20 = make_raw_email(
        "dave@example.com",
        "bob@example.com",
        "Late January",
        "Almost February.",
        "Sat, 20 Jan 2024 10:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &jan1)
        .email(2, true, &jan10)
        .email(3, true, &jan20)
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    // Range [Jan 5, Jan 15) should only include the Jan 10 email.
    let since = chrono::NaiveDate::from_ymd_opt(2024, 1, 5).unwrap();
    let before = chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let emails = client
        .fetch_date_range(&Folder::Inbox, since, before)
        .await
        .unwrap();

    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].from.address, "charlie@example.com");
    assert_eq!(emails[0].subject.original, "Mid January");
}

#[tokio::test]
async fn test_empty_mailbox() {
    let mailbox = MailboxBuilder::new().folder("INBOX").build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    // All operations on an empty folder should return empty results.
    let emails = client.fetch_all(&Folder::Inbox).await.unwrap();
    assert!(emails.is_empty());

    let emails = client.fetch_unseen(&Folder::Inbox).await.unwrap();
    assert!(emails.is_empty());

    let emails = client.fetch_last_n(&Folder::Inbox, 5).await.unwrap();
    assert!(emails.is_empty());
}

// ── Write operation tests ──────────────────────────────────────────

#[tokio::test]
async fn test_add_flag() {
    let raw = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Unread message",
        "Mark me as read.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, false, &raw) // starts unseen
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let writer = writer_for(&server);

    // Add \Seen flag.
    writer
        .add_flag(1, &Folder::Inbox, &Flag::Seen)
        .await
        .unwrap();

    // Verify: fetching unseen should now return nothing.
    let client = client_for(&server);
    let unseen = client.fetch_unseen(&Folder::Inbox).await.unwrap();
    assert!(unseen.is_empty());
}

#[tokio::test]
async fn test_remove_flag() {
    let raw = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Read message",
        "Unmark me.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &raw) // starts seen
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let writer = writer_for(&server);

    // Remove \Seen flag.
    writer
        .remove_flag(1, &Folder::Inbox, &Flag::Seen)
        .await
        .unwrap();

    // Verify: fetching unseen should now return this email.
    let client = client_for(&server);
    let unseen = client.fetch_unseen(&Folder::Inbox).await.unwrap();
    assert_eq!(unseen.len(), 1);
    assert_eq!(unseen[0].uid, 1);
}

#[tokio::test]
async fn test_move_to_folder() {
    let raw = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Move me",
        "Moving to trash.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, false, &raw)
        .folder("Trash")
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let writer = writer_for(&server);

    // Move from INBOX to Trash.
    writer
        .move_to_folder(1, &Folder::Inbox, &Folder::Trash)
        .await
        .unwrap();

    // Verify: INBOX should be empty, Trash should have the email.
    let client = client_for(&server);
    let inbox = client.fetch_all(&Folder::Inbox).await.unwrap();
    assert!(inbox.is_empty());

    let trash = client.fetch_all(&Folder::Trash).await.unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].from.address, "alice@example.com");
}

#[tokio::test]
async fn test_archive() {
    let raw = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "Archive me",
        "Please archive.",
        "Mon, 01 Jan 2024 12:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, false, &raw)
        .folder("Archive")
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let writer = writer_for(&server);

    writer.archive(1, &Folder::Inbox).await.unwrap();

    let client = client_for(&server);
    let inbox = client.fetch_all(&Folder::Inbox).await.unwrap();
    assert!(inbox.is_empty());

    let archive = client.fetch_all(&Folder::Archive).await.unwrap();
    assert_eq!(archive.len(), 1);
}

#[tokio::test]
async fn test_unmark_all_read() {
    let email1 = make_raw_email(
        "alice@example.com",
        "bob@example.com",
        "First read",
        "Was read.",
        "Mon, 01 Jan 2024 10:00:00 +0000",
    );
    let email2 = make_raw_email(
        "charlie@example.com",
        "bob@example.com",
        "Second read",
        "Also read.",
        "Mon, 01 Jan 2024 11:00:00 +0000",
    );

    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .email(1, true, &email1) // seen
        .email(2, true, &email2) // seen
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let writer = writer_for(&server);

    // Before: no unseen emails.
    let client = client_for(&server);
    let unseen = client.fetch_unseen(&Folder::Inbox).await.unwrap();
    assert!(unseen.is_empty());

    // Unmark all as read.
    writer.unmark_all_read(&Folder::Inbox).await.unwrap();

    // After: both emails should be unseen.
    let unseen = client.fetch_unseen(&Folder::Inbox).await.unwrap();
    assert_eq!(unseen.len(), 2);
}
