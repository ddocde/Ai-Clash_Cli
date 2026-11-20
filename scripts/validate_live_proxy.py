#!/usr/bin/env python3
"""Opt-in real-node/systemd/TUN acceptance inside a disposable Docker container.

Requires a prepared service, working remote nodes and an isolated network namespace.
Outputs only sanitized assertions; never prints node identities or captured diagnostics.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import socket
import struct
import urllib.request
import urllib.parse
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir', default='/var/lib/clash-cli')
    parser.add_argument('--group', default='TEST')
    parser.add_argument('--url', default='https://www.gstatic.com/generate_204')
    parser.add_argument('--stream-url', default='https://speed.cloudflare.com/__down?bytes=10485760',
                        help='Bounded streaming request used to observe the selected proxy chain')
    parser.add_argument('--allow-network-changes', action='store_true', required=True,
                        help='Explicitly allow service/TUN changes in this disposable container')
    args = parser.parse_args()
    if not Path('/.dockerenv').exists() or os.geteuid() != 0:
        parser.error('Run as root inside an isolated disposable Docker container')
    if Path('/proc/1/comm').read_text().strip() != 'systemd':
        parser.error('The container must run systemd as PID 1')
    results = []
    probe_route_added = False

    def command(argv, timeout=40, allow_failure=False):
        p = subprocess.run(argv, capture_output=True, text=True, encoding='utf-8', timeout=timeout)
        if p.returncode and not allow_failure:
            raise RuntimeError(f'{Path(argv[0]).name} exited with status {p.returncode}; captured output omitted')
        return p

    def cli(*argv, allow_failure=False):
        p = command(['clash-cli', '--data-dir', args.data_dir, *argv, '--json'], allow_failure=allow_failure)
        # Batch tests may return nonzero with a useful partial result.
        return json.loads(p.stdout)

    def passed(name, **details):
        results.append({'check': name, 'result': 'passed', **details})

    def ready(selection=None):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            p = command(['clash-cli', '--data-dir', args.data_dir, 'node', 'list', '--json'], allow_failure=True)
            if p.returncode == 0:
                proxies = json.loads(p.stdout).get('proxies', {})
                group = proxies.get(args.group)
                if group and (selection is None or group.get('now') == selection):
                    return proxies
            time.sleep(.1)
        raise RuntimeError('Core readiness timeout')

    def curl(proxy=None):
        argv = ['curl', '-sS', '--retry', '2', '--retry-all-errors', '--retry-delay', '1', '--max-time', '20', '-o', '/dev/null', '-w', '%{http_code}']
        argv += ['--noproxy', '', '--proxy', proxy] if proxy else ['--proxy', '', '--noproxy', '*']
        if command(argv + [args.url], timeout=70).stdout.strip() != '204':
            raise RuntimeError('Expected HTTP 204 from the acceptance endpoint')

    def dns_hijack_probe():
        transaction = int.from_bytes(os.urandom(2), 'big')
        query = struct.pack('!HHHHHH', transaction, 0x0100, 1, 0, 0, 0)
        query += b''.join(bytes([len(label)]) + label.encode() for label in ['www', 'gstatic', 'com'])
        query += b'\x00' + struct.pack('!HH', 1, 1)
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                sock.settimeout(3)
                # TEST-NET: no real resolver is expected at this destination.
                sock.sendto(query, ('203.0.113.53', 53))
                response, _ = sock.recvfrom(4096)
            identifier, flags, _, answers, _, _ = struct.unpack('!HHHHHH', response[:12])
            return identifier == transaction and flags & 0x8000 != 0 and flags & 0xf == 0 and answers > 0
        except (OSError, struct.error):
            return False

    def observed_proxy_chain(selected):
        state = json.loads(Path(args.data_dir, 'state.json').read_text())
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        host = urllib.parse.urlparse(args.stream_url).hostname
        initial_request = urllib.request.Request(
            f"http://127.0.0.1:{state.get('controller_port', 9090)}/connections",
            headers={'Authorization': 'Bearer ' + state['secret']})
        with opener.open(initial_request, timeout=3) as response:
            previous_ids = {connection['id'] for connection in (json.load(response).get('connections') or [])}
        process = subprocess.Popen(['curl', '-sS', '--noproxy', '*', '--proxy', '', '--max-time', '20',
                                    '--limit-rate', '1024', '-o', '/dev/null', args.stream_url],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                request = urllib.request.Request(
                    f"http://127.0.0.1:{state.get('controller_port', 9090)}/connections",
                    headers={'Authorization': 'Bearer ' + state['secret']})
                with opener.open(request, timeout=3) as response:
                    connections = (json.load(response).get('connections') or [])
                if any(connection.get('id') not in previous_ids and connection.get('metadata', {}).get('host') == host
                       and selected in (connection.get('chains') or []) and (connection.get('download') or 0) > 0
                       for connection in connections):
                    return
                if process.poll() is not None:
                    break
                time.sleep(.1)
            raise RuntimeError('Selected remote node was not observed carrying the transparent request')
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    def snapshot():
        return {
            'links': json.loads(command(['ip', '-j', 'link']).stdout),
            'routes': json.loads(command(['ip', '-j', 'route', 'show', 'table', 'all']).stdout),
            'rules': json.loads(command(['ip', '-j', 'rule']).stdout),
            'dns': hashlib.sha256(Path('/etc/resolv.conf').read_bytes()).hexdigest(),
        }

    def restored(baseline, tun_names):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            current = snapshot()
            if (not any(link['ifname'] in tun_names for link in current['links'])
                    and current['routes'] == baseline['routes']
                    and current['rules'] == baseline['rules'] and current['dns'] == baseline['dns']):
                return
            time.sleep(.1)
        raise RuntimeError('Network state was not restored before the deadline')

    try:
        proxies = ready()
        cli('tun', 'off')
        if cli('service', 'status')['ActiveState'] != 'active':
            raise RuntimeError('Service is not active')
        passed('systemd active', enabled=command(['systemctl', 'is-enabled', 'clash-cli.service']).stdout.strip())
        nodes = cli('node', 'test', '--all', '--timeout', '8000', '--concurrency', '3', '--url', args.url,
                    allow_failure=True)
        working = [node for node in nodes if node.get('status') == 'ok'
                   and node['name'] in proxies[args.group].get('all', [])]
        if not working:
            raise RuntimeError('No working remote node is available in the test group')
        selected = min(working, key=lambda node: node['delay'])['name']
        passed('real-node latency', tested=len(nodes), available=len(working))
        cli('node', 'select', selected, '--group', args.group)
        port = cli('doctor')['mixed_port']
        cli('mode', 'rule')
        curl(f'http://127.0.0.1:{port}'); passed('HTTP CONNECT HTTPS')
        curl(f'socks5h://127.0.0.1:{port}'); passed('SOCKS5 HTTPS')
        p = command(['clash-cli', '--data-dir', args.data_dir, 'exec', '--', 'curl', '-sS', '--max-time', '25',
                     '-o', '/dev/null', '-w', '%{http_code}', args.url])
        if p.stdout.strip() != '204':
            raise RuntimeError('Child proxy did not return HTTP 204')
        passed('exec child proxy')
        cli('mode', 'global'); cli('node', 'select', selected, '--group', 'GLOBAL')
        curl(f'http://127.0.0.1:{port}'); passed('global mode')
        cli('mode', 'rule')
        cli('service', 'restart'); ready(selected); curl(f'http://127.0.0.1:{port}')
        passed('systemd restart preserves selection')
        original_network = snapshot()
        # Force this TEST-NET probe to have no upstream answer even on networks
        # that transparently intercept outbound DNS. TUN's dport-53 rule must
        # override this main-table guard for the probe to succeed.
        command(['ip', 'route', 'add', 'blackhole', '203.0.113.53/32'])
        probe_route_added = True
        baseline = snapshot()
        if dns_hijack_probe():
            raise RuntimeError('Baseline already intercepts reserved DNS destination; test is not isolated')
        default = json.loads(command(['ip', '-j', 'route', 'show', 'default']).stdout)
        gateway = next(route['gateway'] for route in default if 'gateway' in route)
        cli('tun', 'on', '--exclude', gateway + '/32')
        before_names = {link['ifname'] for link in baseline['links']}
        tun_names = {link['ifname'] for link in snapshot()['links']} - before_names
        if not tun_names:
            raise RuntimeError('TUN device was not created')
        passed('TUN device and routing enabled')
        curl(); passed('transparent HTTPS with proxy variables bypassed')
        observed_proxy_chain(selected); passed('selected remote node observed carrying TUN HTTPS traffic')
        if not dns_hijack_probe():
            raise RuntimeError('TUN DNS hijack did not answer the reserved destination probe')
        passed('TUN DNS hijack answers reserved destination with no valid answer without TUN')
        cli('tun', 'off'); restored(baseline, tun_names)
        passed('TUN disable restores routes rules interfaces and DNS file')
        cli('tun', 'on', '--exclude', gateway + '/32'); curl()
        cli('service', 'stop'); restored(baseline, tun_names)
        passed('systemd stop with active TUN restores network')
        cli('tun', 'off'); cli('service', 'start'); ready(selected); curl(f'http://127.0.0.1:{port}')
        passed('service recovers after TUN shutdown')
        cli('service', 'uninstall')
        if Path('/etc/systemd/system/clash-cli.service').exists() or not Path(args.data_dir, 'state.json').is_file():
            raise RuntimeError('Service uninstall did not preserve the expected state')
        passed('service uninstall removes unit and preserves profiles')
        command(['ip', 'route', 'del', 'blackhole', '203.0.113.53/32'])
        probe_route_added = False
        restored(original_network, tun_names)
        passed('probe guard removed and original network restored')
        print(json.dumps({'result': 'passed', 'checks': results}, indent=2))
    except Exception as error:
        # Error strings from external commands/configurations deliberately never enter output.
        cleanup_succeeded = False
        try:
            cleanup_succeeded = command(['clash-cli', '--data-dir', args.data_dir, 'service', 'stop', '--json'], allow_failure=True).returncode == 0
        except Exception:
            cleanup_succeeded = False
        cleanup_tun_disabled = False
        try:
            cleanup_tun_disabled = command(['clash-cli', '--data-dir', args.data_dir, 'tun', 'off', '--json'], allow_failure=True).returncode == 0
        except Exception:
            cleanup_tun_disabled = False
        probe_route_removed = not probe_route_added
        if probe_route_added:
            try:
                probe_route_removed = command(['ip', 'route', 'del', 'blackhole', '203.0.113.53/32'], allow_failure=True).returncode == 0
            except Exception:
                probe_route_removed = False
        failure = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        print(json.dumps({'result': 'failed', 'checks': results, 'failure': failure, 'cleanup_stop_succeeded': cleanup_succeeded, 'cleanup_tun_disabled': cleanup_tun_disabled, 'cleanup_probe_route_removed': probe_route_removed}, indent=2))
        raise SystemExit(1) from None


if __name__ == '__main__':
    main()
