//! Integration tests for `ProtonClient` using the fake IMAP server.
//!
//! Each test constructs a `Mailbox` with test data, starts a
//! `FakeImapServer` on a random port, creates a `ProtonClient`
//! pointing at it, and exercises one of the client's public methods.

mod fake_imap;

use fake_imap::{FakeImapServer, MailboxBuilder};
use protonmail_client::{ImapConfig, ProtonClient};

/// Build a minimal valid RFC 2822 email.
///
/// The format follows RFC 2822: headers separated by CRLF, a blank
/// line (CRLF CRLF) separating headers from body, and the body text.
fn make_raw_email(
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
    date: &str,
) -> Vec<u8> {
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

/// Create a `ProtonClient` pointed at the fake server.
fn client_for(server: &FakeImapServer) -> ProtonClient {
    let config = ImapConfig {
        host: "127.0.0.1".to_string(),
        port: server.port(),
        username: "testuser".to_string(),
        password: "testpass".to_string(),
    };
    ProtonClient::new(config)
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

    let email = client.fetch_uid("INBOX", 42).await.unwrap();
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
        .email(1, true, &seen_email)   // seen = true -> \Seen flag
        .email(2, false, &unseen_email) // seen = false -> UNSEEN
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    let emails = client.fetch_unseen("INBOX").await.unwrap();

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

    let emails = client.fetch_all("INBOX").await.unwrap();
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
    let emails = client.fetch_last_n("INBOX", 2).await.unwrap();
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
    let emails = client.search("INBOX", "ALL").await.unwrap();
    assert_eq!(emails.len(), 2);
}

#[tokio::test]
async fn test_empty_mailbox() {
    let mailbox = MailboxBuilder::new()
        .folder("INBOX")
        .build();

    let server = FakeImapServer::start(mailbox).await;
    let client = client_for(&server);

    // All operations on an empty folder should return empty results.
    let emails = client.fetch_all("INBOX").await.unwrap();
    assert!(emails.is_empty());

    let emails = client.fetch_unseen("INBOX").await.unwrap();
    assert!(emails.is_empty());

    let emails = client.fetch_last_n("INBOX", 5).await.unwrap();
    assert!(emails.is_empty());
}
