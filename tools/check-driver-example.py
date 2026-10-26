#!/usr/bin/env python3
"""Build the production mouse example with only the public SDK, outside checkouts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    sdk = Path(__file__).resolve().parents[1]
    checkouts = [sdk]
    if (sdk.parent / 'kernel/Cargo.toml').is_file():
        checkouts.append(sdk.parent)
    output = args.output.resolve()
    if any(output == p or p in output.parents for p in checkouts):
        raise ValueError('Output must be outside the SDK and OS checkout')
    # Never replace an earlier proof or release artifact.
    if output.exists():
        raise FileExistsError(output)
    with tempfile.TemporaryDirectory(prefix='osaiku-driver-example-') as temporary:
        root = Path(temporary).resolve()
        if any(root == p or p in root.parents for p in checkouts):
            raise ValueError('TEMP must be outside both checkouts')
        for name in ('driver-protocol', 'driver-runtime', 'targets', 'examples/usb-mouse'):
            shutil.copytree(sdk / name, root / name)
        shutil.copyfile(sdk / 'rust-toolchain.toml', root / 'rust-toolchain.toml')
        (root / 'Cargo.toml').write_text(
            '[workspace]\nmembers=["driver-protocol","driver-runtime"]\n'
            'exclude=["examples/usb-mouse"]\nresolver="2"\n'
            '[workspace.package]\nedition="2021"\nversion="0.1.0"\nlicense="MIT"\n')
        env = dict(os.environ, CARGO_NET_OFFLINE='true', CARGO_TARGET_DIR=str(root / 'target'))
        for key in ('OSAIKU_KERNEL_TARGET', 'OSAIKU_USER_TARGET', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET'):
            env.pop(key, None)
        manifest = 'examples/usb-mouse/Cargo.toml'
        def run(*command):
            subprocess.run(command, cwd=root, env=env, check=True)
        run('cargo', 'generate-lockfile', '--manifest-path', manifest, '--offline')
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--manifest-path', manifest, '--format-version', '1', '--offline', '--locked'],
            cwd=root, env=env, text=True))
        packages = set()
        for package in metadata['packages']:
            if root not in Path(package['manifest_path']).resolve().parents or package['source'] is not None:
                raise ValueError('Dependency escaped isolated public SDK')
            packages.add(package['name'])
        if packages != {'example-usb-mouse', 'osaiku-driver-protocol', 'osaiku-driver-runtime'}:
            raise ValueError('Unexpected package dependency')
        run('cargo', 'build', '--manifest-path', manifest, '--offline', '--locked', '--release',
            '-Zbuild-std=core,compiler_builtins', '-Zbuild-std-features=compiler-builtins-mem',
            '-Zjson-target-spec', '--target', str(root / 'targets/x86_64-osaiku-user.json'))
        elf = root / 'target/x86_64-osaiku-user/release/usb-mouse'
        content = elf.read_bytes()
        if content[:4] != b'\x7fELF':
            raise ValueError('Missing native ELF')
        report = dict(schema='osaiku-driver-example-build-v1', passed=True, independent=True,
                      packages=sorted(packages), elf_sha256=hashlib.sha256(content).hexdigest(),
                      elf_bytes=len(content), qualification='build-only; not installed or executed')
        output.mkdir(parents=True, exist_ok=False)
        shutil.copyfile(elf, output / 'usb-mouse')
        shutil.copyfile(root / 'examples/usb-mouse/driver.toml', output / 'driver.toml')
        (output / 'build.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))


if __name__ == '__main__':
    main()
