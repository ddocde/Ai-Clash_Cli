#!/usr/bin/env python3
"""Validate documented command paths against the built CLI without running operations."""
import pathlib
import re
import shlex
import subprocess
import sys

binary = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/clash-cli').resolve()
root = pathlib.Path(__file__).resolve().parents[1]
count = 0
for name in ('README.md', 'README.zh-CN.md'):
    text = (root / name).read_text(encoding='utf-8')
    for line in text.splitlines():
        if not re.match(r'^(?:sudo(?: --preserve-env=SSH_CONNECTION)? )?(?:\./)?clash-cli ', line):
            continue
        args = shlex.split(line)
        command_index = next(i for i, token in enumerate(args) if token in ('clash-cli', './clash-cli'))
        args = args[command_index + 1:]
        # Validate command path and flags only; --help short-circuits execution.
        if '--' in args:
            args = args[:args.index('--')]
        result = subprocess.run([str(binary), *args, '--help'], capture_output=True, text=True, encoding='utf-8')
        if result.returncode:
            raise SystemExit(f'{name}: invalid command: {line}\n{result.stderr}')
        count += 1
print(f'Validated {count} documented command invocations (help only).')
