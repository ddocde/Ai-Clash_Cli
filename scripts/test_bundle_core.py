"""Release-payload tests; no network or executable invocation."""
import hashlib
import io
import json
import pathlib
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import bundle_core


class BundleTests(unittest.TestCase):
    def build(self, corrupt=False):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            archive = b'fixture archive'
            (root / 'engine.gz').write_bytes(archive)
            (root / 'metadata.json').write_text(json.dumps({
                'tag_name': bundle_core.VERSION,
                'assets': [{'name': f'mihomo-linux-amd64-compatible-{bundle_core.VERSION}.gz',
                            'digest': 'sha256:' + hashlib.sha256(b'corrupt' if corrupt else archive).hexdigest()}],
            }))
            source = io.BytesIO()
            with tarfile.open(fileobj=source, mode='w:gz') as tar:
                for name, content in [('mihomo/vendor/LICENSE', b'wrong'), ('mihomo/LICENSE', b'upstream license')]:
                    entry = tarfile.TarInfo(name)
                    entry.size = len(content)
                    tar.addfile(entry, io.BytesIO(content))
            args = ['bundle_core.py', '--arch', 'x86_64', '--output', str(root / 'out'),
                    '--archive', str(root / 'engine.gz'), '--metadata', str(root / 'metadata.json')]
            with patch.object(sys, 'argv', args), patch.object(bundle_core, 'fetch', return_value=source.getvalue()) as fetch:
                if corrupt:
                    with self.assertRaisesRegex(SystemExit, 'checksum mismatch'):
                        bundle_core.main()
                    fetch.assert_not_called()
                    self.assertFalse((root / 'out').exists())
                else:
                    bundle_core.main()
                    bundle = root / 'out/bundled'
                    self.assertEqual((bundle / 'LICENSE.mihomo').read_bytes(), b'upstream license')
                    self.assertEqual((bundle / 'mihomo.gz').read_bytes(), archive)
                    self.assertEqual((bundle / f'mihomo-{bundle_core.VERSION}-source.tar.gz').read_bytes(), source.getvalue())

    def test_bundle_carries_top_level_license_and_source(self):
        self.build()

    def test_tampered_engine_produces_no_package(self):
        self.build(corrupt=True)


if __name__ == '__main__':
    unittest.main()
