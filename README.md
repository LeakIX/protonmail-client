# protonmail-client

A Rust interface to fetch emails from Proton Mail using
[Proton Bridge](https://proton.me/mail/bridge). This is a read-only IMAP client
that connects over STARTTLS with self-signed certificate support.

The library returns parsed `Email` structs from
[email-parser](https://github.com/LeakIX/email-parser) - it does not implement
its own email types.

- [API Documentation](https://leakix.github.io/protonmail-client)
- [CLI usage](#cli)

## Environment variables

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

## MSRV

The minimum supported Rust version is **1.90.0** (edition 2024).

## License

MIT
