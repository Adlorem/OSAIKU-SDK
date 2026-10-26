#!/usr/bin/env python3
"""Driver authoring/public-key export and independent Rust verification."""
import hashlib
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

CHECKER = Path(sys.argv.pop(1)).resolve()
TOOL = Path(__file__).with_name('package.py')


class DriverPackages(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='driver-package-test-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        key = Ed25519PrivateKey.from_private_bytes(bytes([0x42]) * 32)
        self.public = key.public_key().public_bytes_raw()
        (self.root / 'private').write_bytes(bytes([0x42]) * 32)
        (self.root / 'public').write_bytes(self.public)
        elf = bytearray(64)
        elf[:7] = b'\x7fELF\x02\x01\x01'
        struct.pack_into('<HHI', elf, 16, 2, 62, 1)
        struct.pack_into('<H', elf, 52, 64)
        (self.root / 'driver.elf').write_bytes(elf)
        self.manifest = '''[package]
id = "example.driver"
name = "Driver"
version = "1"
release = 1
epoch = 2
executable = "driver.elf"
memory_bytes = 262144
[compatibility]
abi_major = 1
[driver]
classes = 1
cpu_permille = 50
queue = 8
isolation = "mediated"
reset = "function"
[[matches]]
bus = "usb"
class = 3
'''
        (self.root / 'driver.toml').write_text(self.manifest)

    def run_tool(self, *args, ok=True):
        result = subprocess.run([sys.executable, str(TOOL), '--checker', str(CHECKER), *map(str, args)], cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result

    def approve(self, domain, epoch):
        self.run_tool('trust-add', '--policy', 'policy', '--public-key', 'public', '--fingerprint', hashlib.sha256(self.public).hexdigest(), '--domain', domain, '--epoch', epoch)

    def test_pack_requires_driver_domain_and_sensitive_class_approval(self):
        self.run_tool('driver-pack', 'driver.toml', '--key', 'private', '--output', 'driver.osadrv')
        self.assertIn('verified=false', self.run_tool('driver-inspect', 'driver.osadrv').stdout)
        self.run_tool('inspect', 'driver.osadrv', ok=False)
        self.approve('application', 2)
        self.run_tool('driver-verify', 'driver.osadrv', '--policy', 'policy', ok=False)
        self.approve('driver', 3)
        self.run_tool('driver-verify', 'driver.osadrv', '--policy', 'policy', ok=False)
        self.approve('driver-input', 4)
        # This newly admitted key's minimum epoch is 4. Reissue under that epoch.
        (self.root / 'driver.toml').write_text(self.manifest.replace('epoch = 2', 'epoch = 4'))
        self.run_tool('driver-pack', 'driver.toml', '--key', 'private', '--output', 'current.osadrv')
        self.assertIn('verified=true', self.run_tool('driver-verify', 'current.osadrv', '--policy', 'policy').stdout)
        self.run_tool('trust-revoke', '--policy', 'policy', '--public-key', 'public', '--fingerprint', hashlib.sha256(self.public).hexdigest(), '--domain', 'driver-input', '--epoch', 5)
        self.run_tool('driver-verify', 'current.osadrv', '--policy', 'policy', ok=False)

    def test_unknown_fields_window_and_wrong_extension_leave_no_output(self):
        for manifest in [self.manifest + 'typo = 1\n', self.manifest.replace('release = 1', 'release = 1\nentry = "window"')]:
            (self.root / 'bad.toml').write_text(manifest)
            self.run_tool('driver-pack', 'bad.toml', '--key', 'private', '--output', 'bad.osadrv', ok=False)
            self.assertFalse((self.root / 'bad.osadrv').exists())
        self.run_tool('driver-pack', 'driver.toml', '--key', 'private', '--output', 'bad.osapp', ok=False)
        self.assertFalse((self.root / 'bad.osapp').exists())

    def test_key_requests_are_public_only_and_revocation_is_explicit(self):
        self.run_tool('driver-key', '--public-key', 'public', '--classes', 1, '--output', 'input.osadkey')
        self.assertEqual((self.root / 'input.osadkey').read_bytes(), self.public + struct.pack('<Q', 1))
        self.run_tool('driver-key', '--public-key', 'public', '--classes', 1, '--revoke', '--output', 'revoke.osadkey')
        self.assertEqual(struct.unpack('<Q', (self.root / 'revoke.osadkey').read_bytes()[32:])[0], (1 << 63) | 1)

    def test_repository_and_local_source_have_separate_domain_and_key_scope(self):
        self.run_tool('driver-pack', 'driver.toml', '--key', 'private', '--output', 'driver.osadrv')
        (self.root / 'index.toml').write_text('''[repository]
id = "example.drivers"
sequence = 1
issued_at = 100
expires_at = 1000
epoch = 2
[[entries]]
package = "driver.osadrv"
url = "https://example.invalid/driver.osadrv"
''')
        self.run_tool('index-pack', 'index.toml', '--key', 'private', '--output', 'foreign.index', ok=False)
        self.run_tool('driver-index-pack', 'index.toml', '--key', 'private', '--output', 'driver.index')
        self.run_tool('index-inspect', 'driver.index', ok=False)
        self.run_tool('driver-index-inspect', 'driver.index')
        self.approve('repository', 1)
        args = ('--policy', 'policy', '--now', 150, '--repository', 'example.drivers',
                '--sequence-floor', 0, '--checkpoint', '0' * 64)
        self.run_tool('driver-index-verify', 'driver.index', *args, ok=False)
        self.approve('driver-repository', 2)
        self.run_tool('driver-index-verify', 'driver.index', *args)
        # Repository trust alone cannot authenticate or start the package.
        self.run_tool('driver-verify', 'driver.osadrv', '--policy', 'policy', ok=False)
        config = '''[source]
domain = "driver"
id = "example.drivers"
address = "https://example.invalid/index"
public_key = "public"
channel = "stable"
minimum_epoch = 2
abi_major = 1
'''
        (self.root / 'source.toml').write_text(config)
        self.run_tool('source-pack', 'source.toml', '--output', 'drivers.osadsrc')
        data = (self.root / 'drivers.osadsrc').read_bytes()
        self.assertEqual((len(data), data[:10]), (512, b'OSASRC01\x04\x01'))
        self.run_tool('source-pack', 'source.toml', '--output', 'wrong.osasrc', ok=False)
        (self.root / 'source.toml').write_text(config.replace('https:', 'http:'))
        self.run_tool('source-pack', 'source.toml', '--output', 'bad.osadsrc', ok=False)
        self.assertFalse((self.root / 'bad.osadsrc').exists())


if __name__ == '__main__':
    unittest.main()
