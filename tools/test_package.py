#!/usr/bin/env python3
"""Python packer/Rust verifier interoperability and host policy CLI contracts."""
import hashlib
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

CHECKER = Path(sys.argv.pop(1)).resolve()
TOOL = Path(__file__).with_name("package.py")
VECTORS = TOOL.parents[1] / "package/tests/vectors"


class Packages(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="osaiku-package-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        # TEST ONLY, kept in the temporary directory and never in runtime policy.
        self.seed = bytes([0x42]) * 32
        self.key = Ed25519PrivateKey.from_private_bytes(self.seed)
        self.public = self.key.public_key().public_bytes_raw()
        self.fingerprint = hashlib.sha256(self.public).hexdigest()
        (self.root / "private").write_bytes(self.seed)
        (self.root / "public").write_bytes(self.public)
        elf = bytearray(64)
        elf[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HHI", elf, 16, 2, 62, 1)
        struct.pack_into("<H", elf, 52, 64)
        (self.root / "app.elf").write_bytes(elf)
        self.manifest = '''[package]
id = "dev.example.hello"
name = "Hello"
version = "1.0"
release = 5
epoch = 2
executable = "app.elf"
memory_bytes = 1048576
[compatibility]
abi_major = 1
'''
        (self.root / "app.toml").write_text(self.manifest)

    def run_tool(self, *args, ok=True):
        run = subprocess.run([sys.executable, str(TOOL), "--checker", str(CHECKER), *map(str, args)],
                             cwd=self.root, capture_output=True, text=True)
        self.assertEqual(run.returncode == 0, ok, run.stdout + run.stderr)
        return run

    def pack(self):
        self.run_tool("pack", "app.toml", "--key", "private", "--output", "app.osapp")

    def approve(self, domain="application", epoch=2):
        return self.run_tool("trust-add", "--policy", "policy", "--public-key", "public",
                             "--fingerprint", self.fingerprint, "--domain", domain, "--epoch", epoch)

    def repository(self):
        (self.root / "index.toml").write_text('''[repository]
id = "dev.example.repository"
sequence = 3
issued_at = 100
expires_at = 1000
epoch = 3
[[entries]]
package = "app.osapp"
url = "https://packages.example/hello.osapp"
''')
        self.run_tool("index-pack", "index.toml", "--key", "private", "--output", "index.osaidx")

    def test_fixed_external_vectors_and_no_automatic_trust(self):
        self.pack()
        data = (self.root / "app.osapp").read_bytes()
        self.assertEqual(data, bytes.fromhex((VECTORS / "application.hex").read_text()))
        self.assertIn("verified=false", self.run_tool("inspect", "app.osapp").stdout)
        self.run_tool("verify", "app.osapp", "--policy", "missing", ok=False)
        self.approve()
        self.assertIn("verified=true", self.run_tool("verify", "app.osapp", "--policy", "policy").stdout)
        self.approve("repository", 3)
        self.repository()
        self.assertEqual((self.root / "index.osaidx").read_bytes(), bytes.fromhex((VECTORS / "repository.hex").read_text()))

    def test_signed_category_and_unknown_category_refusal(self):
        manifest = self.manifest.replace('[package]', '[package]\ncategory = "office"')
        (self.root / "app.toml").write_text(manifest)
        self.pack()
        self.approve()
        self.assertIn("category=office", self.run_tool("verify", "app.osapp", "--policy", "policy").stdout)
        (self.root / "app.toml").write_text(manifest.replace('"office"', '"administrator"'))
        self.run_tool("pack", "app.toml", "--key", "private", "--output", "bad.osapp", ok=False)
        self.assertFalse((self.root / "bad.osapp").exists())

    def test_revocation_epoch_and_exact_rollback_approval(self):
        self.pack()
        self.approve()
        self.run_tool("verify", "app.osapp", "--policy", "policy", "--release-floor", 6, ok=False)
        sha = hashlib.sha256((self.root / "app.osapp").read_bytes()).hexdigest()
        self.run_tool("verify", "app.osapp", "--policy", "policy", "--release-floor", 6, "--rollback-digest", sha)
        self.run_tool("trust-revoke", "--policy", "policy", "--public-key", "public", "--fingerprint", self.fingerprint,
                      "--domain", "application", "--epoch", 3)
        self.run_tool("verify", "app.osapp", "--policy", "policy", ok=False)
        self.run_tool("trust-add", "--policy", "policy", "--public-key", "public", "--fingerprint", self.fingerprint,
                      "--domain", "application", "--epoch", 4, ok=False)

    def test_fingerprint_is_explicit_and_failed_policy_edits_are_atomic(self):
        self.run_tool("trust-add", "--policy", "policy", "--public-key", "public", "--fingerprint", "0"*64,
                      "--domain", "application", "--epoch", 2, ok=False)
        self.assertFalse((self.root / "policy").exists())
        self.assertFalse((self.root / "policy.lock").exists())
        self.approve()
        before = (self.root / "policy").read_bytes()
        self.run_tool("trust-revoke", "--policy", "policy", "--public-key", "public", "--fingerprint", self.fingerprint,
                      "--domain", "application", "--epoch", 2, ok=False)
        self.assertEqual(before, (self.root / "policy").read_bytes())

    def test_repository_freshness_and_exact_package_binding(self):
        self.pack()
        self.approve()
        self.approve("repository", 3)
        self.repository()
        common = ("index-verify", "index.osaidx", "--policy", "policy", "--repository", "dev.example.repository")
        self.run_tool(*common, "--now", 500, "--sequence-floor", 0, "--checkpoint", "0"*64, "--package", "app.osapp")
        self.run_tool(*common, "--now", 1000, "--sequence-floor", 0, "--checkpoint", "0"*64, ok=False)
        self.run_tool(*common, "--now", 500, "--sequence-floor", 4, "--checkpoint", "0"*64, ok=False)
        self.run_tool(*common, "--now", 500, "--sequence-floor", 3, "--checkpoint", "0"*64, ok=False)
        checkpoint = hashlib.sha256((self.root / "index.osaidx").read_bytes()).hexdigest()
        self.run_tool(*common, "--now", 500, "--sequence-floor", 3, "--checkpoint", checkpoint)
        (self.root / "app.toml").write_text(self.manifest.replace("release = 5", "release = 6"))
        self.run_tool("pack", "app.toml", "--key", "private", "--output", "different.osapp")
        self.run_tool(*common, "--now", 500, "--sequence-floor", 0, "--checkpoint", "0"*64, "--package", "different.osapp", ok=False)

    def test_noncanonical_inputs_never_publish_or_replace_an_output(self):
        for replacement in ['id = "org.osaiku.themes"', 'id = "../escape"', 'unknown = "x"']:
            (self.root / "app.toml").write_text(self.manifest.replace('id = "dev.example.hello"', replacement))
            self.run_tool("pack", "app.toml", "--key", "private", "--output", "bad.osapp", ok=False)
            self.assertFalse((self.root / "bad.osapp").exists())
        (self.root / "app.toml").write_text(self.manifest)
        self.pack()
        before = (self.root / "app.osapp").read_bytes()
        self.run_tool("pack", "app.toml", "--key", "private", "--output", "app.osapp", ok=False)
        self.assertEqual(before, (self.root / "app.osapp").read_bytes())

    def test_resources_icons_padding_and_hashes(self):
        (self.root / "data").write_bytes(b"abc")
        (self.root / "icon").write_bytes(b"RGBA" + struct.pack("<HH", 2, 2) + b"\xff"*16)
        resources = '''
[[resources]]
name = "assets.data"
path = "data"
[[resources]]
name = "assets.icon"
path = "icon"
kind = "icon"
'''
        (self.root / "app.toml").write_text(self.manifest + resources)
        self.pack()
        self.approve()
        self.run_tool("verify", "app.osapp", "--policy", "policy")
        data = bytearray((self.root / "app.osapp").read_bytes())
        data[-1] ^= 1
        (self.root / "damaged").write_bytes(data)
        self.run_tool("inspect", "damaged", ok=False)
        (self.root / "icon").write_bytes(b"RGBA" + struct.pack("<HH", 65535, 65535))
        self.run_tool("pack", "app.toml", "--key", "private", "--output", "bad.osapp", ok=False)
        self.assertFalse((self.root / "bad.osapp").exists())


if __name__ == "__main__":
    unittest.main()
