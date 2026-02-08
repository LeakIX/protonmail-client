# protonmail-client

Read-only IMAP client library for Proton Mail via
[Proton Bridge](https://proton.me/mail/bridge). Connects over STARTTLS with
self-signed certificate support.

[Documentation](https://leakix.github.io/protonmail-client)

## Library

```rust
use protonmail_client::{ImapConfig, ProtonClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ImapConfig::from_env()?;
    let client = ProtonClient::new(config);

    // List folders
    let folders = client.list_folders().await?;

    // Fetch last 10 emails
    let emails = client.fetch_last_n("INBOX", 10).await?;

    // Search
    let results = client.search("INBOX", "FROM alice@example.com").await?;

    // Fetch single email by UID
    let email = client.fetch_uid("INBOX", 42).await?;

    Ok(())
}
```

### Environment variables

| Variable | Default | Required |
|---|---|---|
| `IMAP_HOST` | `127.0.0.1` | No |
| `IMAP_PORT` | `1143` | No |
| `IMAP_USERNAME` | - | Yes |
| `IMAP_PASSWORD` | - | Yes |

## CLI

The crate includes a `proton-cli` binary for command-line access.

```sh
# List recent emails
proton-cli list --limit 10

# List unseen emails
proton-cli list --unseen

# Show a single email
proton-cli show 42

# List folders
proton-cli folders

# IMAP search
proton-cli search "FROM alice@example.com"

# JSON output (for scripting)
proton-cli list --json --limit 5
```

## License

MIT
