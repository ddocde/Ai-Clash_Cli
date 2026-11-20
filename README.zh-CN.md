# Ai-Clash CLI

[![CI](https://github.com/ddocde/Ai-Clash_Cli/actions/workflows/ci.yml/badge.svg)](https://github.com/ddocde/Ai-Clash_Cli/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/ddocde/Ai-Clash_Cli?include_prereleases)](https://github.com/ddocde/Ai-Clash_Cli/releases) [![License](https://img.shields.io/github/license/ddocde/Ai-Clash_Cli)](LICENSE)

[English](README.md) · [架构说明](docs/architecture.md) · [贡献指南](CONTRIBUTING.md) · [发布维护](docs/maintaining.md)

面向 SSH 会话和无桌面服务器的 Rust 命令行代理管理工具。复用 Mihomo 内核，提供订阅、分享链接、延迟测速、节点选择、路由模式，以及 Linux TUN 和服务管理。

## 平台状态

| 平台 | 状态 |
| --- | --- |
| Linux x86_64 | 已在独立 Ubuntu 24.04 Docker 中通过真实节点、systemd、TUN、DNS 与恢复验证；真实服务器 SSH 场景仍待验收 |
| Linux ARM64 | 已实现内核映射和 CI 打包；未在本地进行运行验收 |
| Windows / macOS | 后续计划；已预留平台边界，尚不支持生产使用 |

CLI 使用 Rust，代理协议由独立安装的 Mihomo 内核处理。本项目独立开发，并非 Clash Verge 或 Mihomo 官方客户端。

## 安装离线发布包

[下载最新预览版](https://github.com/ddocde/Ai-Clash_Cli/releases)，按 CPU 选择 `linux-x86_64`（Intel/AMD）或 `linux-aarch64`（ARM64），校验 `SHA256SUMS` 后完整解压。CLI 使用 musl 静态链接，避免依赖构建机的 glibc 版本。

Linux 发布压缩包包含 `clash-cli`、`bundled/` 中固定的 Mihomo v1.19.34 内核压缩包、内核许可证及对应源码。解压后保留完整目录：

```sh
./clash-cli core install
./clash-cli --help
```

`core install` 校验随包内核并安装到数据目录，**无需联网**。将解压目录加入 PATH 后即可使用下文命令，或使用 `./clash-cli`。服务器无法访问 GitHub 时，可以在其他电脑取得完整发布包，再通过 scp 上传。捆绑解决首次使用时的第二次下载；最初发布包仍需通过可访问的渠道取得。工具不会自动使用第三方 GitHub 镜像。

后续网络可用时，显式更新内核：

```sh
clash-cli core update --version v1.19.34
```

慢速网络可以延长每个请求的总超时，包含响应体下载时间（单位秒，默认 180，允许 1–3600）：

```sh
clash-cli core update --version v1.19.34 --download-timeout 900
```

更新会验证上游 SHA-256 摘要，保留上一份托管内核供 `core rollback` 使用。替换前停止内核，完成后重新启动。也可以选择离线上传的内核：

```sh
clash-cli core install --path /absolute/path/to/mihomo
```

### 从源码构建

`cargo install` 仅构建 CLI，不包含随包内核：

```sh
cargo install --path crates/clash-cli --locked
clash-cli core update
```

离线构建后使用 `core install --path`。发布维护者通过 `scripts/bundle_core.py` 准备随包资源，上游下载和校验发生在打包阶段，无需最终用户的服务器再次下载。

使用 `--data-dir /absolute/path` 指定独立实例的数据目录，后续命令保持一致。不要把私密运行数据放进源码仓库；`doctor` 显示实际目录。

多实例运行时，启动前配置不同的监听端口：

```sh
clash-cli config --mixed-port 17890 --controller-port 19090
```

## 快速开始

```sh
clash-cli profile add main 'https://YOUR-PROVIDER/CLASH-SUBSCRIPTION'
clash-cli profile use main
clash-cli run
```

`run` 保持前台运行。打开另一个 SSH 会话：

```sh
clash-cli status
clash-cli group list
clash-cli node list
clash-cli node test 'NODE-NAME'
clash-cli node select 'NODE-NAME' --group 'GROUP-NAME'
clash-cli exec -- curl https://example.com
```

将节点名和组名替换为查询结果中的完整名称，包含空格时加引号。选择只影响指定代理组。

为当前 Bash 或 Zsh 会话配置代理：

```sh
eval "$(clash-cli env --shell bash)"
curl https://example.com
unset http_proxy https_proxy all_proxy
```

只有遵循代理环境变量的程序才会使用代理。CLI 子进程无法直接修改父 shell；`exec` 仅影响启动的程序。

## 配置与分享链接

```sh
clash-cli profile add local /absolute/path/to/config.yaml
clash-cli profile list
clash-cli profile update main
clash-cli profile use local
clash-cli node import 'YOUR-SHARE-LINK' --profile local
```

只有分享链接时，无需准备 YAML 文件，导入会创建本地配置：

```sh
clash-cli node import 'YOUR-SHARE-LINK' --profile personal
clash-cli profile use personal
```

支持 `ss://`、`vmess://`、`vless://`、`trojan://` 链接。不支持的参数明确报错，并不保证兼容所有服务商扩展。建议将手动节点导入本地配置，避免订阅刷新替换。链接和订阅地址等同密码，命令参数可能留在 shell 历史或进程列表中。

添加配置前先安装内核：导入会调用实际内核进行校验。

本地覆盖项与订阅分开存储，配置启用前先经过内核校验。更换订阅来源前备份重要配置。

## 延迟测速

```sh
clash-cli node test 'NODE-NAME'
clash-cli node test --group 'GROUP-NAME'
clash-cli node test --all --concurrency 4 --timeout 5000
```

批量测速会输出已取得的全部结果；任一节点失败时退出码非零，应逐项读取结果，不要丢弃部分成功输出。

`--timeout` 单位为毫秒，`--url` 可指定测试地址。结果反映访问测试地址的延迟，不是下载带宽。

## 路由模式与 TUN

```sh
clash-cli mode rule
clash-cli mode global
clash-cli mode direct
```

`global` 将进入内核的流量交给 GLOBAL 组，不会自动接管整台服务器。根据需要在 GLOBAL 组中选择节点或代理组。TUN 才负责系统流量接管，需要网络权限。内核停止时，`tun on` 只保存下次启动使用的设置，不会启动内核；启动 root 服务后才实际接管流量。

特权操作必须使用固定的绝对数据目录，并排除实际 SSH 管理网段：

```sh
sudo --preserve-env=SSH_CONNECTION clash-cli --data-dir /var/lib/clash-cli tun on --exclude 192.0.2.0/24
sudo clash-cli --data-dir /var/lib/clash-cli tun off
```

示例网段仅用于文档，必须替换为实际管理 CIDR。先在同一目录安装内核、导入并启用配置。sudo 可能清除 SSH 环境变量；保留 `SSH_CONNECTION` 或明确提供管理网段。首次特权测试请在隔离环境或具备控制台恢复方式的机器上进行。

## 后台服务

先安装 root 所有的 CLI（服务安装会拒绝普通用户可写的路径）：

```sh
sudo install -m 755 ./clash-cli /usr/local/bin/clash-cli
sudo mkdir -p /usr/local/lib/clash-cli
sudo cp -R ./bundled /usr/local/lib/clash-cli/
```

先用 Ctrl-C 停止前面的普通用户前台实例，释放端口，再初始化服务专用实例：

```sh
sudo clash-cli --data-dir /var/lib/clash-cli core install
sudo clash-cli --data-dir /var/lib/clash-cli profile add main 'https://YOUR-PROVIDER/CLASH-SUBSCRIPTION'
sudo clash-cli --data-dir /var/lib/clash-cli profile use main
```

`service install` 默认启用开机自启动，`service start` 立即启动服务。`service stop` 只停止当前实例，保留开机自启；如需仅关闭自启而保留服务文件，可执行 `sudo systemctl disable clash-cli.service`。

然后执行：

```sh
sudo clash-cli --data-dir /var/lib/clash-cli service install
sudo clash-cli --data-dir /var/lib/clash-cli service start
sudo clash-cli --data-dir /var/lib/clash-cli service status
sudo clash-cli --data-dir /var/lib/clash-cli logs
sudo clash-cli --data-dir /var/lib/clash-cli service stop
sudo clash-cli --data-dir /var/lib/clash-cli service uninstall
```

管理 root 实例时，状态查询和选节点同样使用该前缀：

```sh
sudo clash-cli --data-dir /var/lib/clash-cli status
sudo clash-cli --data-dir /var/lib/clash-cli node list
```

安装服务前将 CLI 放在稳定路径；服务引用期间不要删除 CLI 或内核。卸载服务不会自动删除配置。无 systemd 环境可由自己的进程管理器托管 `run`。

## 自动化与排障

使用 `clash-cli COMMAND --help` 查看参数，使用 `--json` 获取机器可读结果。失败返回非零退出码，错误写入 stderr。`env`、`exec`、前台内核输出和日志属于流式命令。

- **无法连接内核**：运行 `status`、`doctor`，检查内核是否启动、端口是否占用。
- **订阅被拒绝**：确认链接返回 Clash/Mihomo YAML，而不是网页登录页面。
- **找不到节点或组**：订阅更新后重新查询列表。
- **TUN 权限或设备错误**：检查 sudo、`/dev/net/tun` 和容器网络权限。
- **更新内核**：显式执行 `core update --version VERSION`；`core rollback` 恢复可用的上一份托管内核。替换内核后重启。

提交问题时不要附带真实订阅地址或控制 API 密钥。

## AI Skill

可复用技能位于 [skills/clash-cli/SKILL.md](skills/clash-cli/SKILL.md)。Codex 用户可将 `skills/clash-cli` 目录复制到 `~/.codex/skills/`，然后提出：

> 使用 $clash-cli 检查代理状态，测试节点，并为我指定的代理组选一个可用的低延迟节点。

Skill 会读取实际帮助和 JSON 输出，检查平台能力，并区分只读诊断与网络状态修改。

## 验证

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_docs.py target/debug/clash-cli
python3 scripts/smoke_real_core.py /absolute/path/to/mihomo
```

可选的真实内核测试需要 curl，以及空闲的回环端口 17890、19090 和 18081。测试使用临时数据，不启用 TUN、不安装服务，验证 HTTP 代理、重载、选择持久化与回退。真实节点的特权 Docker 验收见[容器验证说明](docs/validation/docker.md)及脱敏的 16 项报告。[扩展验收](docs/validation/startup-recovery.md)新增 39 项通过记录，覆盖容器自启与恢复、真实 SSH 连续性、订阅、UDP/IPv6 代理、双实例及在线更新回退。物理服务器重启与原生 IPv6 外网仍待验收。

## 开发与许可

参见[贡献指南](CONTRIBUTING.md)、[架构说明](docs/architecture.md)和[第三方声明](THIRD_PARTY.md)。项目采用 GPL-3.0-only；内核和依赖保留各自声明。不要提交真实凭据或订阅文件。
