#!/usr/bin/env python3
"""Build an isolated public Driver Kit consumer; never mount/import the OS.

The temporary directory must be outside the SDK and OS checkout. All dependency
paths in Cargo metadata must remain in that isolated directory. --target checks
the public no_std x86_64 target using the pinned nightly rust-src component.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', action='store_true')
    parser.add_argument('--guest-output', type=Path)
    parser.add_argument('--oversize', action='store_true')
    args = parser.parse_args()
    sdk = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix='osaiku-driver-kit-') as temporary:
        root = Path(temporary).resolve()
        checkouts = [sdk]
        if (sdk.parent/'kernel/Cargo.toml').is_file():
            checkouts.append(sdk.parent)
        if any(root == checkout or checkout in root.parents for checkout in checkouts):
            raise RuntimeError('Independent build TEMP must be outside the OS/SDK checkout')
        for name in ('driver-protocol', 'driver-runtime'):
            shutil.copytree(sdk/name, root/name)
        shutil.copytree(sdk/'tests/driver-kit-client', root/'client')
        shutil.copytree(sdk/'targets', root/'targets')
        shutil.copyfile(sdk/'rust-toolchain.toml', root/'rust-toolchain.toml')
        (root/'Cargo.toml').write_text('[workspace]\nmembers=["driver-protocol","driver-runtime"]\nexclude=["client"]\nresolver="2"\n[workspace.package]\nedition="2021"\nversion="0.1.0"\nlicense="MIT"\n')
        env = dict(os.environ, CARGO_NET_OFFLINE='true', CARGO_TARGET_DIR=str(root/'target'))
        # Inherited OS target overrides are deliberately excluded from this proof.
        env.pop('OSAIKU_KERNEL_TARGET', None)
        env.pop('OSAIKU_USER_TARGET', None)
        def run(*command):
            subprocess.run(command, cwd=root, env=env, check=True)
        run('cargo', 'generate-lockfile', '--manifest-path', 'client/Cargo.toml', '--offline')
        metadata = json.loads(subprocess.check_output(['cargo','metadata','--manifest-path','client/Cargo.toml',
            '--format-version','1','--offline','--locked'], cwd=root, env=env, text=True))
        names = set()
        for package in metadata['packages']:
            path = Path(package['manifest_path']).resolve()
            if root not in path.parents or package['source'] is not None:
                raise RuntimeError('Private/external dependency escaped the isolated Driver Kit')
            names.add(package['name'])
        assert names == {'driver-kit-client', 'osaiku-driver-runtime', 'osaiku-driver-protocol'}
        run('cargo','build','--manifest-path','client/Cargo.toml','--release','--offline','--locked')
        run('cargo','test','--manifest-path','client/Cargo.toml','--offline','--locked')
        if args.target:
            run('cargo','build','--manifest-path','client/Cargo.toml','--release','--offline','--locked',
                '-Zbuild-std=core,compiler_builtins','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec','--features','oversize' if args.oversize else 'guest','--target',str(root/'targets/x86_64-osaiku-user.json'))
            if args.guest_output:
                args.guest_output.resolve().parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(root/'target/x86_64-osaiku-user/release/driver-process-fixture', args.guest_output.resolve())
        print(json.dumps(dict(passed=True, independent=True, packages=sorted(names), no_std_target=args.target)))


if __name__ == '__main__':
    main()
