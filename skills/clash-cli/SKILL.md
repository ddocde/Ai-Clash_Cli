---
name: clash-cli
description: Operate the clash-cli headless Mihomo manager to manage profiles, test and select proxy nodes, diagnose connectivity, and manage Linux service or TUN settings. Use for this CLI, not unrelated desktop Clash clients.
---

# clash-cli

Discover the installed interface with `clash-cli --version`, `clash-cli --help`, and relevant subcommand help. Use `doctor --json` to inspect platform and data directory; keep the same explicit `--data-dir` throughout a workflow, especially across sudo. Windows/macOS system integration is planned, not assumed supported.

## Diagnose and select

1. Read `status --json`, `profile list --json`, `group list --json`, and `node list --json` as needed. A connection error means the engine is unavailable, not an empty node list.
2. Test the user's target group with `node test --group 'GROUP' --json`, or an exact node with `node test 'NODE' --json`. Use `--all` only when appropriate; it generates traffic. Timeout is milliseconds; latency is not download throughput.
3. Select an actual member of the intended group using `node select 'NODE' --group 'GROUP'`. Do not switch unrelated groups or routing mode to make a test pass.
4. Re-read state to verify the selection. Check nonzero exit codes; do not treat an error message or partial batch result as success.

## Configure

Use `core install` for a verified offline bundle shipped with a release, `core update --version VERSION` for an explicitly requested online download, or `core install --path /absolute/path/to/mihomo` for an existing trusted engine. Keep the bundle adjacent to the CLI or under the installation prefix at `lib/clash-cli/bundled`. Cargo builds do not include it; if missing, use an offline path or explicit online update. For slow downloads, `core update --download-timeout 900` extends each metadata/archive request deadline in seconds; the default is 180 and the CLI range is 1–3600. A timeout leaves the installed binary unchanged. Do not silently upgrade versions. Install an engine before importing profiles because imports validate with the engine.

Import Clash/Mihomo YAML with `profile add NAME SOURCE`, then `profile use NAME`. Share links use `node import LINK --profile NAME`; use a local profile so subscription updates do not overwrite manually imported nodes. Never disclose the full source URL, links, passwords or controller secret in summaries, logs, fixtures or commits. Avoid reading private config contents when list/status output is enough.

`run` is foreground execution. A background systemd service is installed and operated through `service`; use root-owned `/usr/local/bin/clash-cli` and `/var/lib/clash-cli`; executable, core, state and ancestors must not be user-writable. Initialize this directory through sudo, stop any earlier user instance occupying the same ports, and use the same sudo/data-dir prefix for later management. `service install` enables boot startup but does not start immediately; `service stop` leaves startup enabled. Changes to service installation, network capture or the selected node must stay within the user's requested scope. Existing authorization remains valid; do not introduce repetitive approval prompts.

## Proxy semantics and TUN

- `mode global` only affects traffic already entering the engine; it does not capture all host traffic.
- `env --shell bash` prints assignments for the caller to evaluate. The CLI cannot mutate a parent shell. `exec -- PROGRAM ARGS` supplies proxy variables to a child; only cooperating applications honor them.
- When stopped, `tun on` only saves next-start configuration; start the privileged engine before claiming capture is active. When running, it changes host routing and requires privileges. Before enabling, inspect platform capability and management connectivity. Under sudo preserve `SSH_CONNECTION` or supply the real management CIDR with `--exclude`; never copy a documentation-only CIDR as a real route.
- Verify status and connectivity after changes. On a failed activation, inspect the reported rollback result before further mutation. Use `tun off` with the same data directory to disable capture when authorized. Do not claim SSH continuity, DNS recovery or route cleanup without observing it.

Use the installed help as the authority when versions differ. Stop on unsupported capabilities or missing credentials, report the concrete limitation, and do not improvise firewall or service changes outside the CLI.

Example requests: “Test my proxy group and select the fastest working node”; “检查代理状态并排查服务器无法联网的原因”。
