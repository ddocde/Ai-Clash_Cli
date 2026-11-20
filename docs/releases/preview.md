# v0.1.2 Linux preview / Linux 预览版

A headless Rust CLI for Mihomo with subscriptions, share-link import, latency tests, node selection, routing modes, Linux TUN and systemd services.

## Changes / 更新

- Add `core update --download-timeout SECONDS` (default 180; range 1–3600) for slow downloads. / 支持自定义内核下载超时。
- Reject empty, unknown-only and provider-error-only payloads before modifying saved or running state. / 订阅错误响应不会覆盖已有配置。
- Normalize YAML merge keys before applying listener/controller/DNS restrictions; retain original source bytes. / 修复 YAML 继承字段绕过运行配置限制的问题。
- Let fresh instances choose unused ports while another proxy owns the defaults. / 支持默认端口已占用时配置独立实例。
- Clarify boot startup: `service install` enables systemd startup, `service start` runs now, `service stop` retains startup. / 明确服务安装即启用开机自启。

## Offline packages / 离线安装包

Choose `linux-x86_64` for Intel/AMD or `linux-aarch64` for ARM64. Each archive includes the static Rust CLI, pinned Mihomo **v1.19.34**, its license and corresponding source. Check `SHA256SUMS`, extract the entire archive, then run `./clash-cli core install` without network access.

选择对应 CPU 架构，校验并完整解压后执行 `./clash-cli core install`，无需再次下载内核。更新须主动执行；停止内核后再替换，必要时使用 `core rollback`。

## Validation / 验证

50 Rust tests and strict Clippy pass locally. CI checks Linux, Windows and macOS; release automation verifies real-engine offline installation and proxy/restart/rollback smoke tests for both Linux package architectures. Compilation is not Windows/macOS runtime acceptance.

The initial [16 Docker checks](https://github.com/ddocde/Ai-Clash_Cli/blob/main/docs/validation/docker.md) are supplemented by [39 extended checks](https://github.com/ddocde/Ai-Clash_Cli/blob/main/docs/validation/startup-recovery.md): normal/unclean container boot, saved TUN/selection, core/parent crash recovery, real SSH session continuity, network reconnect, real subscription refresh, non-DNS UDP, IPv6 proxying, independent concurrent instances and actual v1.19.33 → v1.19.34 → v1.19.33 update/rollback.

新增 39 项扩展验证，覆盖自启、异常恢复、SSH、订阅、UDP/IPv6 代理、双实例和跨版本更新回退。报告已脱敏。

Physical host reboot, native IPv6 uplink, other distributions and long-duration load remain unverified. Trojan/Hysteria2 samples were unreachable, and SS/VMess lacked live samples. Windows/macOS native integration is planned. Use different ports alongside Clash Verge and enable only one client's TUN. This is a prerelease.
