"""The independent Driver Kit must not silently regain private dependencies."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('kit', Path(__file__).with_name('check-driver-kit.py'))
kit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kit)


class Dependencies(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.packages = [dict(name=name, version='0.1.0', source=None,
                             manifest_path=str(self.root/name/'Cargo.toml'))
                         for name in ('driver-kit-client', 'osaiku-driver-runtime',
                                      'osaiku-driver-protocol', 'osaiku-network-protocol',
                                      'osaiku-wifi-protocol')]
        self.packages.append(dict(name='zeroize', version='1.8.2',
            source='registry+https://github.com/rust-lang/crates.io-index',
            manifest_path=str(self.root.parent/'registry/zeroize/Cargo.toml')))

    def test_complete_public_graph(self):
        self.assertEqual(len(kit.validate_packages(self.packages, self.root)), 6)

    def test_private_path_cannot_escape(self):
        self.packages[0]['manifest_path'] = str(self.root.parent/'private/Cargo.toml')
        with self.assertRaisesRegex(RuntimeError, 'Private'):
            kit.validate_packages(self.packages, self.root)

    def test_registry_exception_is_exact(self):
        original = self.packages[-1].copy()
        for key, value in [('name', 'unknown'), ('version', '1.8.1'),
                           ('source', None), ('source', 'git+https://example.invalid/zeroize')]:
            with self.subTest(key=key, value=value):
                self.packages[-1] = dict(original, **{key: value})
                with self.assertRaisesRegex(RuntimeError, 'Unapproved'):
                    kit.validate_packages(self.packages, self.root)

    def test_missing_or_duplicate_dependency_is_refused(self):
        for packages in (self.packages[:-1], self.packages + self.packages[:1]):
            with self.assertRaises(RuntimeError):
                kit.validate_packages(packages, self.root)


if __name__ == '__main__':
    unittest.main()
