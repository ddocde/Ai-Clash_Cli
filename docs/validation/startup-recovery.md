# Startup and extended acceptance — 2026-11-20

The [sanitized report](extended-report.json) records **39 additional unique passing checks** in an isolated Ubuntu 24.04 x86_64 Docker container. Initial startup/recovery tests used v0.1.1; fixes and final regressions used the v0.1.2 candidate. This supplements the [initial 16-check acceptance](docker.md), rather than replacing it.

## Boot startup

After installing the engine and selecting a profile in the same data directory:

```sh
sudo clash-cli --data-dir /var/lib/clash-cli service install
sudo clash-cli --data-dir /var/lib/clash-cli service start
sudo clash-cli --data-dir /var/lib/clash-cli service status
```

`service install` installs and **enables** the systemd unit for subsequent boots. `service start` starts it immediately. `service stop` stops the current service while leaving boot startup enabled. Disable boot startup with `sudo systemctl disable clash-cli.service`; use `service uninstall` to remove the unit. The current CLI has no separate enable/disable subcommand.

A saved TUN setting and node selection survived both a normal container cold restart and `docker kill --signal KILL` followed by `docker start`, without a manual service start. This tests systemd boot inside a container, not firmware, host boot ordering or a physical server reboot.

## Evidence

| Area | Result |
| --- | --- |
| Startup, recovery, SSH | 11 checks: enabled startup, saved TUN/selection, core/parent crash recovery, existing and new SSH sessions, bridge disconnect/reconnect and clean TUN removal |
| Real HTTPS subscription | 5 checks: import 90 nodes, refresh, failed-refresh preservation/redaction, activation and active refresh with selection retained |
| Non-DNS UDP | 2 checks: valid STUN binding response and controller evidence of the selected remote node carrying UDP bytes |
| IPv6 proxy | 5 checks: IPv4 control, forced IPv6 HTTPS over SOCKS, IPv6 TUN routes/traffic and route restoration |
| Online core update | 5 checks: verified v1.19.33 download, failed-download preservation, verified v1.19.34 update, live traffic and working cross-version rollback |
| Two instances | 4 checks: alternate ports, concurrent HTTPS, separate controller secrets/state and independent shutdown |
| Routing modes | 3 checks: DOMAIN-SUFFIX plus REJECT rules, global selection and direct mode |
| Profile safety | 3 checks: inherited YAML bindings sanitized in the running core, rejected provider error envelope, continued proxy connectivity |
| Offline profile import | 1 check: actual provider YAML imports into fresh local state without network bootstrap |

The UDP probe used Cloudflare's public [STUN endpoint](https://developers.cloudflare.com/realtime/turn/) and verified a binding response; it did not test HTTP/3 or a TURN allocation. The IPv6 test used a node with IPv6 egress over IPv4 transport. A temporary ULA on loopback allowed the engine's IPv6 interface check in this IPv4-only Docker network; it was removed afterward. Native IPv6 uplink acceptance remains pending.

Three Trojan and four Hysteria2 samples were unreachable in this environment. Those protocol checks are recorded as **not verified**, not passing. No SS/VMess live samples were available. Multi-NIC management, other distributions, ARM64 privileged networking, Windows/macOS native integration, physical host reboot and long-duration load still need dedicated acceptance.

## Repeating the tests

Start with the isolation, private input handling and root ownership requirements in [Docker validation](docker.md). Use systemd as PID 1, a private cgroup namespace, writable private cgroups, `/dev/net/tun`, `NET_ADMIN` and `SYS_ADMIN`; no host network, host PID namespace or host filesystem mount is needed. Set Docker's stop signal to `SIGRTMIN+3` with a 30-second stop timeout for the orderly restart case.

Keep subscription credentials outside the repository. Inspect service state, authenticated controller selection, network routes and a real HTTPS request after each restart. For SSH, connect from outside the container and enable/disable TUN through the **same** SSH TCP session while preserving `SSH_CONNECTION`. A successful new SSH login alone does not prove continuity. The crash and network-disconnect cases require separate probes after recovery.

The one-off extended orchestration used private provider inputs and is not shipped. The public repeatable baseline is `scripts/validate_live_proxy.py`; the JSON report supplies the extended observed outcomes. Neither report contains subscription URLs or node credentials. The host's Clash Verge settings and selections were not changed.

中文说明：开机自启已实现，安装服务即启用自启，`start` 立即运行，`stop` 不关闭自启。本次新增 39 项通过记录涵盖容器冷启动、异常终止恢复、真实 SSH 会话连续性、订阅、UDP、IPv6 代理、双实例与跨版本更新回退。容器启动不等同于物理服务器开机验收；原生 IPv6 外网、不可达的协议样本和长时间负载仍未验收。
