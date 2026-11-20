## Linux preview / Linux 预览版

A headless Rust CLI for Mihomo: import subscriptions or share links, test latency, select nodes and manage routing. / 面向无桌面服务器的 Rust 代理管理工具，支持订阅和链接导入、延迟测速、节点选择与路由管理。

### Offline packages / 离线安装包

- Choose `linux-x86_64` for Intel/AMD 64-bit or `linux-aarch64` for ARM64.
- Each archive includes the statically linked CLI, pinned Mihomo **v1.19.34**, its license and corresponding source.
- Compare the archive SHA-256 with `SHA256SUMS`, extract the whole package and run `./clash-cli core install`. This command needs no network.
- 选择对应 CPU 架构的安装包，校验 `SHA256SUMS` 后完整解压，执行 `./clash-cli core install`，无需再次下载内核。

See the packaged English/Chinese README for subscription setup and usage. Updates are explicit: `core update --version VERSION`; stop the engine before replacing it.

### Validation and limits / 验证与限制

Release automation runs CI on Linux, Windows and macOS, plus real-engine offline installation, proxy, restart-persistence and rollback tests on both Linux architectures. Cross-platform compilation does not imply Windows/macOS runtime support.

**v0.1.1 fixes:** built-in policies no longer cause false failures in all-node tests, empty node sets fail clearly, and service lifecycle commands avoid a configuration-lock startup race.

**Preview:** real-node TUN/systemd/DNS/recovery passed 16 checks in isolated Ubuntu 24.04 Docker. Native server SSH continuity and IPv6 acceptance remain pending. See [validation evidence](https://github.com/ddocde/Ai-Clash_Cli/blob/main/docs/validation/docker.md). Do not enable two proxy clients' TUN simultaneously. Use distinct listener ports when coexisting with Clash Verge. / TUN 和系统服务已通过独立 Docker 的真实节点验证，真实服务器 SSH/IPv6 场景仍待验收；与其他代理共存时请分配不同端口，且只开启一个 TUN。

Windows/macOS native integration and broader network acceptance are planned. This release is marked as a prerelease accordingly.
