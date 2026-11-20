# Real-node Docker acceptance

This is an opt-in network test using a disposable Linux container, a trusted local image, and a private Clash/Mihomo profile. It changes the container's service, routing and TUN state. The image must provide systemd, bash, mount, iproute2, curl, Python 3 and getent. The host must provide Docker and `/dev/net/tun`.

## Prepare

Build the CLI (`cargo build --workspace`) and prepare `bundled/` using `scripts/bundle_core.py` or an extracted Linux release. Place a private profile outside the repository. It must contain standalone working remote nodes, a `select` group named `TEST`, and a final `MATCH,TEST` rule. Avoid providers/rules requiring unrelated downloads for this test. Use at most a few nodes to bound test traffic.

Create the container with separate network and cgroup namespaces. NET_ADMIN is needed for TUN; SYS_ADMIN and the AppArmor setting allow systemd to manage the container's private cgroup mount. The test does not need host filesystem mounts or published ports.

```sh
docker run -d --name ai-clash-validation --user 0 \
  --cap-add NET_ADMIN --cap-add SYS_ADMIN --device /dev/net/tun \
  --cgroupns private --security-opt apparmor=unconfined \
  --tmpfs /run --tmpfs /run/lock --tmpfs /tmp --env container=docker \
  --entrypoint /bin/bash YOUR_LOCAL_IMAGE \
  -c 'mount -o remount,rw /sys/fs/cgroup && exec /sbin/init'

docker exec ai-clash-validation mkdir -p /usr/local/lib/clash-cli /root/private
docker cp target/debug/clash-cli ai-clash-validation:/usr/local/bin/clash-cli
docker cp /PATH/TO/bundled ai-clash-validation:/usr/local/lib/clash-cli/bundled
docker cp /PRIVATE/PATH/nodes.yaml ai-clash-validation:/root/private/nodes.yaml
docker cp scripts/validate_live_proxy.py ai-clash-validation:/root/private/validate_live_proxy.py
```

Copied files can retain ownership/permissions from the source. Set trusted ownership inside the container before installing a root service:

```sh
docker exec ai-clash-validation chown -R root:root /usr/local/bin/clash-cli /usr/local/lib/clash-cli /root/private
docker exec ai-clash-validation chmod 755 /usr/local/bin/clash-cli
docker exec ai-clash-validation chmod 700 /root/private
docker exec ai-clash-validation chmod 600 /root/private/nodes.yaml
docker exec ai-clash-validation clash-cli --data-dir /var/lib/clash-cli core install
docker exec ai-clash-validation clash-cli --data-dir /var/lib/clash-cli profile add validation /root/private/nodes.yaml
docker exec ai-clash-validation clash-cli --data-dir /var/lib/clash-cli profile use validation
docker exec ai-clash-validation clash-cli --data-dir /var/lib/clash-cli service install
docker exec ai-clash-validation clash-cli --data-dir /var/lib/clash-cli service start
```

The input remains private container data. Never commit it, place it in an image layer, or include it in a bug report.

## Run and clean up

```sh
docker exec ai-clash-validation python3 /root/private/validate_live_proxy.py --allow-network-changes
docker rm -f ai-clash-validation
```

The script selects an available node using latency results. Some candidate nodes may be unavailable; that is reported as a count rather than exposing identities. It verifies HTTP CONNECT, SOCKS5, exec, global mode, service restart persistence, TUN transparent HTTPS, actual selected-node traffic through a newly observed controller connection, DNS interception, and route/interface/rule/DNS-file recovery on disable and stop. It finally uninstalls the service while checking that profiles remain.

The DNS probe temporarily installs a main-table blackhole route for one TEST-NET destination, preventing an upstream resolver from answering directly. It requires no valid answer before TUN and a valid answer through TUN's dport-53 policy and DNS hijack. The guard route is removed and the original network snapshot is verified at completion. The stream probe requests at most 10 MiB from Cloudflare, terminates as soon as a new connection carries received bytes through the chosen node, and is not a download speed benchmark. Other requests use the public gstatic HTTP 204 endpoint. Keep the default endpoints available, or supply a compatible `--url` / `--stream-url`.

Output is a sanitized JSON report: check names, counts and booleans. External command output, controller credentials, node names and endpoints are omitted. On failure the script attempts to stop the service, disable its saved TUN setting and remove the probe guard; remove the disposable container after inspection to delete copied private data. Keep the original private input under your own control.

## Acceptance record — 2026-11-20

An isolated Ubuntu 24.04 container was tested with the pinned Mihomo v1.19.34 engine and three real subscription-derived candidates. One VLESS node was available; two candidates were unavailable. Test names were anonymized and credentials were retained only in private temporary storage.

All 16 live checks passed on the source candidate identified in the report; subsequent compatible dependency security patches are separately covered by unit tests and CI. See [the sanitized report](docker-report.json). The selected-node TUN connection and the guarded DNS query both passed. Separate regression checks confirm that `service status` works while the configuration writer lock is held, configuration mutations remain excluded, and three spaced service restarts complete with `NRestarts=0`. Rapid repeated restarts can intentionally hit systemd's normal start-rate limit; that is distinct from the fixed configuration-lock race.

This verifies the container environment. Native server SSH continuity, IPv6, other distributions, reboot recovery, and Windows/macOS runtime integration still need their own acceptance. The host's Clash Verge settings and node selections were not changed.

中文说明：本测试仅在独立容器内使用真实节点，验证代理、systemd、TUN、DNS 接管和网络恢复。输入配置属于私密数据，报告不包含节点凭据。Docker 中通过不等同于所有服务器或真实 SSH 连接场景均已验收。
