# Changelog

## 0.1.1 — Docker acceptance fixes

- Patch locked rustls and optional quinn-proto dependencies to versions resolving the initial GitHub security advisories.

- Exclude Mihomo built-in routing policies from `node test --all` using API types; preserve real nodes with policy-like names.
- Return an error for empty latency-test selections rather than reporting empty success.
- Retry only transient Linux executable-busy errors during verified offline core installation, with a strict attempt limit and preserved rollback state.
- Remove configuration writer locks from systemd lifecycle/status commands, resolving a parent/child startup race.
- Add opt-in isolated Docker acceptance and a sanitized real-node report covering HTTP/SOCKS, systemd, TUN routing evidence, DNS hijack and network restoration.

## 0.1.0 — Linux preview

- Rust CLI with subscription and local profile management, SS/VMess/VLESS/Trojan import, node selection and bounded concurrent latency tests.
- Config validation, authenticated controller ownership checks, atomic persistence, writer locking and failed-update recovery.
- Offline Linux release bundles with Mihomo v1.19.34, upstream license and corresponding source; explicit `core update` and rollback.
- Linux systemd/TUN interfaces, management route exclusions and graceful termination. Privileged end-to-end acceptance remains pending.
- English and Chinese documentation, reusable AI skill, multi-platform CI and automated Linux releases.

Windows/macOS runtime integration is planned. TUN/systemd are experimental in this preview.
