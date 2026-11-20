#!/usr/bin/env python3
"""Build an offline release payload; verify the upstream engine before packaging."""
import argparse
import hashlib
import io
import json
import pathlib
import re
import tarfile
import urllib.request

VERSION = 'v1.19.34'


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'clash-cli-release'})
    with urllib.request.urlopen(request, timeout=180) as response:
        return response.read()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arch', required=True, choices=['x86_64', 'aarch64'])
    parser.add_argument('--output', required=True, type=pathlib.Path)
    parser.add_argument('--archive', type=pathlib.Path, help='Use a cached engine archive; still verify upstream digest')
    parser.add_argument('--source-archive', type=pathlib.Path, help='Use cached corresponding upstream source')
    parser.add_argument('--metadata', type=pathlib.Path, help='Use cached release metadata')
    args = parser.parse_args()
    # Keep the release payload and Rust installer pinned to the same engine.
    installer = (pathlib.Path(__file__).resolve().parents[1] / 'crates/clash-core/src/installer.rs').read_text()
    if f'pub const DEFAULT_VERSION: &str = "{VERSION}";' not in installer:
        raise SystemExit('Bundle version differs from installer DEFAULT_VERSION')
    platform = {'x86_64': 'linux-amd64-compatible', 'aarch64': 'linux-arm64'}[args.arch]
    asset = f'mihomo-{platform}-{VERSION}.gz'
    base = 'https://api.github.com/repos/MetaCubeX/mihomo/releases/tags/' + VERSION
    metadata = json.loads(args.metadata.read_bytes() if args.metadata else fetch(base))
    if metadata.get('tag_name') != VERSION:
        raise SystemExit('Release metadata version mismatch')
    selected = next(a for a in metadata['assets'] if a['name'] == asset)
    digest = selected.get('digest', '')
    if not re.fullmatch(r'sha256:[0-9a-fA-F]{64}', digest):
        raise SystemExit('Upstream SHA-256 digest missing or malformed')
    archive = args.archive.read_bytes() if args.archive else fetch(
        f'https://github.com/MetaCubeX/mihomo/releases/download/{VERSION}/{asset}')
    if 'sha256:' + hashlib.sha256(archive).hexdigest() != digest.lower():
        raise SystemExit('Engine checksum mismatch')
    # Ship corresponding upstream source and its license together with the binary.
    source_url = f'https://codeload.github.com/MetaCubeX/mihomo/tar.gz/refs/tags/{VERSION}'
    print('Preparing corresponding upstream source...', flush=True)
    source = args.source_archive.read_bytes() if args.source_archive else fetch(source_url)
    with tarfile.open(fileobj=io.BytesIO(source), mode='r:gz') as tar:
        member = next(m for m in tar.getmembers() if len(m.name.split('/')) == 2 and m.name.split('/')[-1] == 'LICENSE' and m.isfile())
        handle = tar.extractfile(member)
        if handle is None:
            raise SystemExit('Upstream LICENSE missing')
        license_bytes = handle.read()
    bundle = args.output / 'bundled'
    bundle.mkdir(parents=True, exist_ok=True)
    (bundle / 'mihomo.gz').write_bytes(archive)
    (bundle / 'manifest.json').write_text(json.dumps({
        'version': VERSION, 'asset': asset, 'digest': digest,
    }, indent=2) + '\n')
    (bundle / 'LICENSE.mihomo').write_bytes(license_bytes)
    (bundle / f'mihomo-{VERSION}-source.tar.gz').write_bytes(source)
    (bundle / 'SOURCE.txt').write_text(f'Mihomo {VERSION}\n{source_url}\nSource archive is included alongside this notice.\n')
    print(f'Bundled {asset} and corresponding upstream source in {bundle}')


if __name__ == '__main__':
    main()
