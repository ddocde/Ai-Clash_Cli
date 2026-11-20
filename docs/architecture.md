# Architecture and platform support

clash-cli manages a separately installed Mihomo engine. It does not implement proxy protocols or embed a desktop runtime. The CLI translates commands into profile transactions, local controller requests, and platform operations.

## Boundaries

- Business operations own profiles, local overrides, node import and selection.
- The controller client authenticates requests to a loopback-only Mihomo API.
- Platform operations own directories, service lifecycle, permissions, and TUN preflight.
- The installer resolves an explicit engine version and platform asset; updates are intentional rather than automatic on startup.

Keep native APIs and operating-system commands behind platform interfaces. Do not use sudo, systemctl, /proc or Unix signals in new shared logic. Unsupported capabilities must be explicit errors, not no-op success.

## Adding a platform

Implement its directory conventions, engine asset mapping, process lifecycle and capability reporting first. Add native service integration (Windows Service or macOS launchd), privilege handling and TUN checks independently. A system-proxy backend is distinct from shell environment output and TUN routing.

Never infer runtime support from compilation. Before marking a platform supported, verify installation, startup, graceful stop, permission errors, configuration persistence, service uninstall, DNS behavior and routing restoration on that OS. Add native CI tests and update both README support matrices.

## Operational invariants

Profile source data and locally managed settings are separate. Validate generated configuration with the actual engine before activation. Keep a previous valid state when an update or reload fails. Controller credentials and subscription tokens are private; diagnostics must not echo full subscription sources.

Global routing mode chooses how traffic already received by Mihomo is handled. Shell proxy variables affect cooperating programs. TUN captures traffic through an operating-system virtual interface and needs network privileges. These are separate controls.
