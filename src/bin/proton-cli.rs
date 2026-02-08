#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

//! CLI for querying Proton Mail via Proton Bridge (read-only)

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use protonmail_client::{Email, ImapConfig, ProtonClient};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "proton-cli")]
#[command(
    about = "Read-only CLI for Proton Mail via Proton Bridge"
)]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List emails
    List {
        /// Folder to list from
        #[arg(long, default_value = "INBOX")]
        folder: String,

        /// Maximum number of emails to show
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Show only unseen emails
        #[arg(long)]
        unseen: bool,

        /// Show emails since this date (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date)]
        since: Option<NaiveDate>,

        /// Show emails before this date (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date)]
        before: Option<NaiveDate>,
    },

    /// Show a single email by UID
    Show {
        /// Email UID
        uid: u32,

        /// Folder containing the email
        #[arg(long, default_value = "INBOX")]
        folder: String,
    },

    /// List available IMAP folders
    Folders,

    /// Search emails using an IMAP search query
    Search {
        /// IMAP search query (e.g. "FROM foo@bar.com")
        query: String,

        /// Folder to search in
        #[arg(long, default_value = "INBOX")]
        folder: String,

        /// Maximum number of results
        #[arg(long, default_value = "50")]
        limit: usize,
    },
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| format!("Invalid date '{s}': {e}"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let config = ImapConfig::from_env()?;
    let client = ProtonClient::new(config);

    match &args.command {
        Command::List {
            folder,
            limit,
            unseen,
            since,
            before,
        } => {
            cmd_list(
                &client, &args, folder, *limit, *unseen, *since,
                *before,
            )
            .await?;
        }
        Command::Show { uid, folder } => {
            cmd_show(&client, &args, folder, *uid).await?;
        }
        Command::Folders => {
            cmd_folders(&client, &args).await?;
        }
        Command::Search {
            query,
            folder,
            limit,
        } => {
            cmd_search(&client, &args, folder, query, *limit)
                .await?;
        }
    }

    Ok(())
}

async fn cmd_list(
    client: &ProtonClient,
    args: &Args,
    folder: &str,
    limit: usize,
    unseen: bool,
    since: Option<NaiveDate>,
    before: Option<NaiveDate>,
) -> anyhow::Result<()> {
    let emails = if unseen {
        client.fetch_unseen(folder).await?
    } else if let Some(since_date) = since {
        let before_date = before.unwrap_or_else(|| {
            chrono::Utc::now().date_naive()
                + chrono::Duration::days(1)
        });
        client
            .fetch_date_range(folder, since_date, before_date)
            .await?
    } else {
        client.fetch_last_n(folder, limit).await?
    };

    let display: Vec<&Email> = emails.iter().take(limit).collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&display)?);
    } else {
        print_email_table(&display);
    }

    Ok(())
}

async fn cmd_show(
    client: &ProtonClient,
    args: &Args,
    folder: &str,
    uid: u32,
) -> anyhow::Result<()> {
    let email = client.fetch_uid(folder, uid).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&email)?);
    } else {
        print_email_detail(&email);
    }

    Ok(())
}

async fn cmd_folders(
    client: &ProtonClient,
    args: &Args,
) -> anyhow::Result<()> {
    let folders = client.list_folders().await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&folders)?);
    } else {
        for folder in &folders {
            println!("{folder}");
        }
    }

    Ok(())
}

async fn cmd_search(
    client: &ProtonClient,
    args: &Args,
    folder: &str,
    query: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let emails = client.search(folder, query).await?;
    let display: Vec<&Email> = emails.iter().take(limit).collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&display)?);
    } else {
        print_email_table(&display);
    }

    Ok(())
}

fn print_email_table(emails: &[&Email]) {
    if emails.is_empty() {
        println!("No emails found.");
        return;
    }

    let header = format!(
        "{:<8} {:<20} {:<30} {}",
        "UID", "Date", "From", "Subject"
    );
    println!("{header}");
    println!("{}", "-".repeat(100));

    for email in emails {
        println!(
            "{:<8} {:<20} {:<30} {}",
            email.uid,
            email.date.format("%Y-%m-%d %H:%M"),
            truncate(&email.from.to_string(), 28),
            truncate(&email.subject.original, 40),
        );
    }

    println!("\n{} email(s)", emails.len());
}

fn print_email_detail(email: &Email) {
    println!("UID:     {}", email.uid);
    println!("Date:    {}", email.date.format("%Y-%m-%d %H:%M:%S"));
    println!("From:    {}", email.from);
    println!(
        "To:      {}",
        email
            .to
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    if !email.cc.is_empty() {
        println!(
            "CC:      {}",
            email
                .cc
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    println!("Subject: {}", email.subject.original);
    println!("Msg-ID:  {}", email.message_id);

    if email.thread.is_reply {
        println!(
            "Reply-To-ID: {}",
            email
                .thread
                .in_reply_to
                .as_ref()
                .map_or("-", |id| id.as_str())
        );
    }

    println!("\n--- Body ---\n");
    println!("{}", email.body.best_text());

    if !email.extracted.emails.is_empty() {
        println!("\n--- Extracted Emails ---");
        for e in &email.extracted.emails {
            println!("  {}", e.address);
        }
    }

    if !email.extracted.urls.is_empty() {
        println!("\n--- Extracted URLs ---");
        for u in &email.extracted.urls {
            println!("  {}", u.url);
        }
    }

    if !email.extracted.phone_numbers.is_empty() {
        println!("\n--- Extracted Phones ---");
        for p in &email.extracted.phone_numbers {
            println!("  {}", p.raw);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String =
            s.chars().take(max.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}
