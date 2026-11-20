# Ai-Clash CLI

[![CI](https://github.com/ddocde/Ai-Clash_Cli/actions/workflows/ci.yml/badge.svg)](https://github.com/ddocde/Ai-Clash_Cli/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/ddocde/Ai-Clash_Cli?include_prereleases)](https://github.com/ddocde/Ai-Clash_Cli/releases) [![License](https://img.shields.io/github/license/ddocde/Ai-Clash_Cli)](LICENSE)

[简体中文](README.zh-CN.md) · [Architecture](docs/architecture.md) · [Contributing](CONTRIBUTING.md) · [Release maintenance](docs/maintaining.md)

A Rust command-line manager for the Mihomo proxy engine, designed for SSH sessions and servers without a desktop. Supports subscriptions, share links, latency checks, node selection, routing modes and Linux TUN/service management. No GUI is required.

## Platform status

| Platform | Status |
| --- | --- |
| Linux x86_64 | Real-node systemd/TUN/DNS/recovery verified in isolated Ubuntu 24.04 Docker; native-host SSH acceptance pending |
| Linux ARM64 | Implemented asset mapping and CI packaging; not locally runtime-tested |
| Windows / macOS | Planned; platform boundaries reserved, not production-supported |

The CLI is Rust; proxy protocol implementation remains in the separately installed Mihomo engine. This is an independent project, not an official Clash Verge or Mihomo client.

## Install the offline release package

[Download the latest preview](https://github.com/ddocde/Ai-Clash_Cli/releases). Choose `linux-x86_64` (Intel/AMD) or `linux-aarch64` (ARM64), verify `SHA256SUMS`, then extract the complete archive. The CLI is statically linked with musl to avoid a dependency on the build runner’s glibc version.

Linux release archives include `clash-cli`, a pinned Mihomo v1.19.34 archive in `bundled/`, its license, and corresponding upstream source. Keep the extracted directory together:

```sh
./clash-cli core install
./clash-cli --help
```

`core install` verifies and installs the bundled engine into the data directory **without network access**. Put the extracted directory on PATH to use the remaining examples, or run `./clash-cli`. Transfer the whole archive from another computer via scp if the server cannot reach GitHub. Bundling removes the second download; obtaining the initial release still requires a reachable distribution channel. No third-party proxy mirror is configured automatically.

For an explicit online update (after connectivity is available):

```sh
clash-cli core update --version v1.19.34
```

On a slow connection, extend the per-request timeout (seconds, including response body; default 180, range 1–3600):

```sh
clash-cli core update --version v1.19.34 --download-timeout 900
```

Updates verify the upstream SHA-256 digest and retain a previous managed binary for `core rollback`. Stop the engine before replacement, then restart it. An offline engine may also be selected:

```sh
clash-cli core install --path /absolute/path/to/mihomo
```

### Build from source

`cargo install` builds the CLI only; it does not bundle the engine:

```sh
cargo install --path crates/clash-cli --locked
clash-cli core update
```

Use `core install --path` when building offline. Release maintainers run `scripts/bundle_core.py` to construct the bundled payload; it downloads and verifies upstream artifacts during packaging, not on the user's server.

Use `--data-dir /absolute/path` consistently to select a separate instance. Do not put private state in this source repository. `doctor` reports the effective location.

For multiple instances, configure distinct listener ports before starting:

```sh
clash-cli config --mixed-port 17890 --controller-port 19090
```

## Quick start

```sh
clash-cli profile add main 'https://YOUR-PROVIDER/CLASH-SUBSCRIPTION'
clash-cli profile use main
clash-cli run
```

`run` stays in the foreground. Open another SSH session for the following commands:

```sh
clash-cli status
clash-cli group list
clash-cli node list
clash-cli node test 'NODE-NAME'
clash-cli node select 'NODE-NAME' --group 'GROUP-NAME'
clash-cli exec -- curl https://example.com
```

Replace node and group names with exact names returned by the list commands. Quote names containing spaces. Node choice applies to its group; it does not change every proxy group at once.

For the current Bash or Zsh session:

```sh
eval "$(clash-cli env --shell bash)"
curl https://example.com
unset http_proxy https_proxy all_proxy
```

Only applications honoring proxy environment variables use this mechanism. A child CLI cannot directly change its parent shell. `exec` sets variables only for the launched process.

## Profiles and individual links

```sh
clash-cli profile add local /absolute/path/to/config.yaml
clash-cli profile list
clash-cli profile update main
clash-cli profile use local
clash-cli node import 'YOUR-SHARE-LINK' --profile local
```

For a share-link-only setup, no existing YAML file is needed:

```sh
clash-cli node import 'YOUR-SHARE-LINK' --profile personal
clash-cli profile use personal
```

Supported share-link families: `ss://`, `vmess://`, `vless://`, `trojan://`. Unsupported link options fail explicitly; arbitrary provider-specific variants are not guaranteed. Import into a local profile so subscription refreshes do not replace manual nodes. Treat links and subscription URLs as passwords; command-line arguments may be visible in process listings and shell history.

Install the core before adding a profile: imports are validated with the actual engine.

Local overrides are separate from downloaded profiles. Configuration activation validates with the engine first. Keep backups of valuable profiles before changing providers.

## Latency checks

```sh
clash-cli node test 'NODE-NAME'
clash-cli node test --group 'GROUP-NAME'
clash-cli node test --all --concurrency 4 --timeout 5000
```

Batch checks print all available results and return nonzero if any node fails; inspect each result rather than discarding partial output.

Timeout is in milliseconds; `--url` selects the test endpoint. These checks measure latency, not download bandwidth.

## Routing and TUN

```sh
clash-cli mode rule
clash-cli mode global
clash-cli mode direct
```

`global` routes traffic received by Mihomo through the GLOBAL group. It does **not** capture all server traffic. Select the desired node/group in GLOBAL as needed. TUN captures system traffic and requires network privileges. When the engine is stopped, `tun on` saves configuration for the next startup; it does not start an engine. Start the root service to actually capture traffic.

Use a fixed absolute data directory for privileged operations and retain the SSH management network as an exclusion:

```sh
sudo --preserve-env=SSH_CONNECTION clash-cli --data-dir /var/lib/clash-cli tun on --exclude 192.0.2.0/24
sudo clash-cli --data-dir /var/lib/clash-cli tun off
```

sudo may strip SSH metadata; preserve `SSH_CONNECTION` or explicitly supply the management CIDR. The example network is documentation-only: replace it with your actual management CIDR. Initialize the same data directory with an engine and active profile first. Do not switch data directories between setup and service commands. TUN preflight checks the device and management connection; perform first privileged validation from an isolated host or with console recovery access.

## Background service

Install a root-owned CLI executable first (service installation rejects user-writable paths):

```sh
sudo install -m 755 ./clash-cli /usr/local/bin/clash-cli
sudo mkdir -p /usr/local/lib/clash-cli
sudo cp -R ./bundled /usr/local/lib/clash-cli/
```

Stop the earlier user foreground instance with Ctrl-C first to free its ports. Initialize the dedicated service instance:

```sh
sudo clash-cli --data-dir /var/lib/clash-cli core install
sudo clash-cli --data-dir /var/lib/clash-cli profile add main 'https://YOUR-PROVIDER/CLASH-SUBSCRIPTION'
sudo clash-cli --data-dir /var/lib/clash-cli profile use main
```

`service install` enables startup at boot; `service start` starts the service immediately. `service stop` stops the current instance while leaving boot startup enabled. To disable boot startup without removing the unit, use `sudo systemctl disable clash-cli.service`.

Then:

```sh
sudo clash-cli --data-dir /var/lib/clash-cli service install
sudo clash-cli --data-dir /var/lib/clash-cli service start
sudo clash-cli --data-dir /var/lib/clash-cli service status
sudo clash-cli --data-dir /var/lib/clash-cli logs
sudo clash-cli --data-dir /var/lib/clash-cli service stop
sudo clash-cli --data-dir /var/lib/clash-cli service uninstall
```

Manage this root instance with the same prefix, including node selection and status:

```sh
sudo clash-cli --data-dir /var/lib/clash-cli status
sudo clash-cli --data-dir /var/lib/clash-cli node list
```

Install the CLI in a stable location before installing its service. Do not delete the CLI/core while a service references it. Service uninstall does not imply deleting your profiles. Without systemd, use `run` under your own supervisor.

## Automation and troubleshooting

Use `clash-cli COMMAND --help` for arguments and `--json` for machine-readable results. Failures return a nonzero status and diagnostics on stderr. `env`, `exec`, foreground engine output and journal logs are stream-oriented commands.

- **Cannot connect:** run `status` and `doctor`; start the core and check for occupied ports.
- **Subscription rejected:** verify it is Clash/Mihomo YAML rather than a web login page.
- **Unknown node/group:** refresh list output after a subscription update.
- **TUN permission/device error:** verify sudo, `/dev/net/tun`, and container network capabilities.
- **Engine update:** use `core update --version VERSION` explicitly; `core rollback` restores an available prior managed binary. Restart after replacing the binary.

Latency measures the configured test URL, not download throughput. Do not post subscription URLs or controller secrets in bug reports.

## AI skill

The reusable agent skill is [skills/clash-cli/SKILL.md](skills/clash-cli/SKILL.md). For Codex, copy the `skills/clash-cli` directory into `~/.codex/skills/`, then ask, for example:

> Use $clash-cli to inspect proxy status, test my nodes and select the fastest working node in my chosen group.

The skill uses actual CLI help and JSON output, checks platform capabilities, and distinguishes diagnosis from changes to network state.

## Validation

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_docs.py target/debug/clash-cli
python3 scripts/smoke_real_core.py /absolute/path/to/mihomo
```

The opt-in smoke requires curl and free loopback ports 17890, 19090 and 18081. It uses temporary data, never enables TUN or installs a service, and tests HTTP proxying, reload, selection persistence and rollback. Real-node privileged Docker acceptance is documented in [Docker validation](docs/validation/docker.md), including a sanitized 16-check report. [Extended acceptance](docs/validation/startup-recovery.md) adds 39 passing checks for container startup/recovery, real SSH continuity, subscriptions, UDP/IPv6 proxying, dual instances and online core update/rollback. Physical host reboot and native IPv6 uplink remain pending.

## Development and license

See [CONTRIBUTING.md](CONTRIBUTING.md), [architecture](docs/architecture.md), and [third-party notices](THIRD_PARTY.md). GPL-3.0-only; upstream engine and dependencies retain their own notices. No real credentials or subscription files belong in commits.
