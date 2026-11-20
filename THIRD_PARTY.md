# Third-party software

This project is independently developed and is not affiliated with Clash Verge Rev or MetaCubeX.

- [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev): architecture reference for profile, core and service management; GPL-3.0. Its desktop interface and Tauri runtime are not bundled.
- [Mihomo](https://github.com/MetaCubeX/mihomo): proxy engine, bundled in Linux release archives under its upstream license. The `bundled/` directory contains the engine license, corresponding source archive, exact version and SHA-256 manifest. Explicit CLI updates download it separately.
- Rust dependency versions are recorded in Cargo.lock. Dependencies retain their respective licenses.

See LICENSE for the license of clash-cli. Engine binaries and upstream source archives are prepared during release packaging and are not checked into this repository.

Architecture reference snapshot: Clash Verge Rev commit
`3f7af9ab022d04ebc45d96b56b58cd67e90fe90d` (2026-11-19), including
`src-tauri/src/core/manager/mod.rs`. The manager's explicit lifecycle/readiness
handling informs the separation between CLI commands, engine control, and services.
