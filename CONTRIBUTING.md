# Contributing

Please open an issue describing a reproducible problem or proposed behavior before a large change. Never include subscription credentials, API secrets, private node URLs, or live configurations in issues or fixtures.

Use Rust Edition 2024. Keep platform-specific code inside the platform adapter; shared business logic must not call systemctl, sudo, or Unix signal APIs. New platform support must declare capabilities honestly and return explicit errors for unsupported operations.

Before submitting a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Update both README languages, CLI help, and the AI skill when commands change. Include tests for observable behavior, especially parsing, configuration preservation, error propagation, and recovery. Network or privileged integration tests must be opt-in and run in an isolated environment.

A new OS backend requires native verification of process lifecycle, permissions, service installation/removal, and TUN routing/DNS restoration before claiming full support. Passing a cross-platform build alone is insufficient.

Contributions are licensed under GPL-3.0-only, the project license. Preserve upstream notices for any reused code.
