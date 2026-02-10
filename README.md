# protonmail-client

[![CI](https://github.com/LeakIX/protonmail-client/actions/workflows/ci.yaml/badge.svg)](https://github.com/LeakIX/protonmail-client/actions/workflows/ci.yaml)
[![crates.io](https://img.shields.io/crates/v/protonmail-client.svg)](https://crates.io/crates/protonmail-client)
[![docs.rs](https://docs.rs/protonmail-client/badge.svg)](https://docs.rs/protonmail-client)

A Rust interface to fetch emails from Proton Mail using
[Proton Bridge](https://proton.me/mail/bridge). This is a read-only IMAP client
that connects over STARTTLS with self-signed certificate support.

The library returns parsed `Email` structs from
[email-extract](https://crates.io/crates/email-extract) - it does not implement
its own email types.

- [API Documentation](https://docs.rs/protonmail-client)
- [crates.io](https://crates.io/crates/protonmail-client)
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
cargo run --release --features cli --bin proton-cli -- list --limit 10

# List unseen emails
cargo run --release --features cli --bin proton-cli -- list --unseen

# Show a single email
cargo run --release --features cli --bin proton-cli -- show 42

# List folders
cargo run --release --features cli --bin proton-cli -- folders

# IMAP search
cargo run --release --features cli --bin proton-cli -- search "FROM alice@example.com"

# JSON output (for scripting)
cargo run --release --features cli --bin proton-cli -- list --json --limit 5
```

## MSRV

The minimum supported Rust version is **1.90.0** (edition 2024).

## License

MIT
