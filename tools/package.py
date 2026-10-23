#!/usr/bin/env python3
"""OSAIKU v1 packages: host authoring with the shared Rust validation gate.

Python 3.11+ and cryptography are required for signing; inspect/verify need only
Python and the package-check executable built from this SDK. No runtime keys.
"""
import argparse
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DOMAINS = {"application": (1, b"OSAPPK01", b"OSAIKU/application-package/v1\0"),
           "repository": (2, b"OSAIDX01", b"OSAIKU/repository-index/v1\0"),
           "system": (3, b"OSASYS01", b"OSAIKU/system-release/v1\0")}
PACKAGE_LIMIT = 256 * 1024 * 1024


def read(path, limit):
    with Path(path).open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"input exceeds {limit} bytes: {path}")
    return data


def load(path):
    return tomllib.loads(read(path, 1024 * 1024).decode("utf-8"))


def fields(value, allowed, required=()):
    if not isinstance(value, dict) or set(value) - set(allowed) or set(required) - set(value):
        raise ValueError("unknown or missing manifest fields")


def fixed(value, size):
    if not isinstance(value, str):
        raise ValueError("text field must be a string")
    data = value.encode("utf-8")
    if not data or len(data) > size or b"\0" in data:
        raise ValueError("invalid text length or embedded NUL")
    return data.ljust(size, b"\0")


def checker(args, *command):
    result = subprocess.run([str(args.checker), *map(str, command)], capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "shared validator refused input")
    return result.stdout


def atomic(path, data, validate, replace=False):
    """Validate the temporary file before publication; never leave a partial file."""
    path = Path(path)
    if path.is_symlink() or (path.exists() and not replace):
        raise ValueError(f"output already exists or is a symlink: {path}")
    fd, name = tempfile.mkstemp(prefix=".osaiku-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(data)
            out.flush()
            os.fsync(out.fileno())
        validate(Path(name))
        if replace:
            os.replace(name, path)
        else:
            # Link creation is no-clobber even if a competing writer raced the
            # initial check. Both names are on the same filesystem.
            os.link(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


def signing_key(path):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    path = Path(path).resolve()
    # Private material belongs outside both SDK and any containing OS checkout.
    for parent in (ROOT, *ROOT.parents):
        if (parent / ".git").exists() and path.is_relative_to(parent):
            raise ValueError("private key must be outside repository checkouts")
    data = read(path, 32)
    if len(data) != 32:
        raise ValueError("private key must be a raw 32-byte seed")
    return Ed25519PrivateKey.from_private_bytes(data)


def envelope(payload, kind, epoch, key):
    if epoch < 1:
        raise ValueError("policy epoch must be positive")
    _, magic, domain = DOMAINS[kind]
    public = key.public_key().public_bytes_raw()
    prefix = struct.pack("<8sIIQQ32s32sQ24x", magic, 1, 192, 192 + len(payload), epoch,
                         hashlib.sha256(public).digest(), hashlib.sha256(payload).digest(), len(payload))
    return prefix + key.sign(domain + prefix) + payload


def compatibility(config):
    fields(config, ("architecture", "abi_major", "abi_minor", "cpu_features", "protocols"), ("abi_major",))
    protocols = config.get("protocols", [])
    if len(protocols) > 32:
        raise ValueError("too many protocols")
    records = []
    for p in protocols:
        fields(p, ("id", "major", "minor", "optional"), ("id", "major"))
        if type(p.get("optional", False)) is not bool:
            raise ValueError("optional must be boolean")
        records.append((p["id"], struct.pack("<IHHII", p["id"], p["major"], p.get("minor", 0),
                                           int(not p.get("optional", False)), 0)))
    records.sort()
    return struct.pack("<HHHHQH14x", 1, config.get("architecture", 62), config["abi_major"],
                       config.get("abi_minor", 0), config.get("cpu_features", 0), len(records)) + b"".join(r[1] for r in records)


def pack(config, base, key):
    fields(config, ("package", "compatibility", "resources"), ("package", "compatibility"))
    p = config["package"]
    fields(p, ("id", "name", "version", "sdk", "release", "epoch", "executable", "permissions",
               "memory_bytes", "io_bytes", "threads", "instances", "surfaces", "entry", "width", "height", "category"),
           ("id", "name", "version", "release", "epoch", "executable", "memory_bytes"))
    compat = compatibility(config["compatibility"])
    resources = config.get("resources", [])
    if len(resources) > 128:
        raise ValueError("too many resources")
    components = [(1, "executable", read(base / p["executable"], 128 * 1024 * 1024))]
    content_bytes = len(components[0][2])
    for resource in sorted(resources, key=lambda r: r["name"]):
        fields(resource, ("name", "path", "kind"), ("name", "path"))
        kind = {"data": 2, "icon": 3}[resource.get("kind", "data")]
        data = read(base / resource["path"], 16 * 1024 * 1024 if kind == 2 else 262152)
        content_bytes += len(data)
        if content_bytes > PACKAGE_LIMIT:
            raise ValueError("package exceeds limit")
        components.append((kind, resource["name"], data))
    table = (512 + len(compat) + 7) & ~7
    metadata_size = table + 112 * len(components)
    metadata = bytearray(metadata_size)
    entry = {"native": 1, "window": 2}[p.get("entry", "native")]
    categories = ("other", "system", "utilities", "office", "graphics", "multimedia",
                  "internet", "games", "development", "education")
    category = categories.index(p.get("category", "other"))
    struct.pack_into("<8sIIQQQQHHHHHHHH", metadata, 0, b"OSAMNF01", 2 if "category" in p else 1, metadata_size,
                     p["release"], p.get("permissions", 0), p["memory_bytes"], p.get("io_bytes", 65536),
                     p.get("threads", 1), p.get("instances", 1), p.get("surfaces", 0), entry,
                     p.get("width", 0), p.get("height", 0), len(compat), len(components))
    metadata[64:192] = fixed(p["id"], 128)
    metadata[192:320] = fixed(p["name"], 128)
    metadata[320:384] = fixed(p["version"], 64)
    metadata[384:448] = fixed(p.get("sdk", "0.1.0"), 64)
    struct.pack_into("<H", metadata, 448, category)
    metadata[512:512 + len(compat)] = compat
    body = bytearray()
    for index, (kind, name, content) in enumerate(components):
        offset = 192 + len(metadata) + len(body)
        padding = (-offset) % 8
        body.extend(b"\0" * padding)
        offset += padding
        if offset + len(content) > PACKAGE_LIMIT:
            raise ValueError("package exceeds limit")
        struct.pack_into("<H6xQQ32s56s", metadata, table + index * 112, kind, offset, len(content),
                         hashlib.sha256(content).digest(), fixed(name, 56))
        body.extend(content)
    return envelope(bytes(metadata) + body, "application", p["epoch"], key)


def pack_index(args, config, base, key):
    fields(config, ("repository", "entries"), ("repository",))
    r = config["repository"]
    fields(r, ("id", "sequence", "issued_at", "expires_at", "epoch"), ("id", "sequence", "issued_at", "expires_at", "epoch"))
    entries = config.get("entries", [])
    if len(entries) > 1024:
        raise ValueError("too many index entries")
    records = []
    for entry in entries:
        fields(entry, ("package", "url"), ("package", "url"))
        path = base / entry["package"]
        data = read(path, PACKAGE_LIMIT)
        # The index binds precisely these bytes. The shared parser checks this
        # snapshot, not a second read of a pathname that could have changed.
        with tempfile.NamedTemporaryFile(delete=False) as snapshot:
            snapshot.write(data)
            name = snapshot.name
        try:
            checker(args, "inspect", name)
        finally:
            Path(name).unlink(missing_ok=True)
        record = (data[32:64] + data[256:384]
                  + struct.pack("<QQ", struct.unpack_from("<Q", data, 208)[0], len(data))
                  + hashlib.sha256(data).digest() + data[706:712] + b"\0\0" + fixed(entry["url"], 296))
        records.append(record)
    records.sort(key=lambda b: b[:160])
    payload = (fixed(r["id"], 128) + struct.pack("<QQQI4x", r["sequence"], r["issued_at"], r["expires_at"], len(records))
               + b"".join(records))
    return envelope(payload, "repository", r["epoch"], key)


def trust(args):
    if args.policy.exists():
        checker(args, "policy", args.policy)
        data = read(args.policy, 4128)
        epoch = struct.unpack_from("<Q", data, 16)[0]
        records = [bytearray(data[i:i+64]) for i in range(32, len(data), 64)]
    else:
        if args.command == "trust-revoke":
            raise ValueError("policy does not exist")
        epoch, records = 0, []
    if args.epoch <= epoch:
        raise ValueError("policy epoch must increase")
    public = read(args.public_key, 32)
    if len(public) != 32 or hashlib.sha256(public).hexdigest() != args.fingerprint:
        raise ValueError("explicit fingerprint approval does not match public key")
    domain = DOMAINS[args.domain][0]
    found = next((r for r in records if r[:32] == public and r[32] == domain), None)
    if args.command == "trust-revoke":
        if found is None:
            raise ValueError("key is not approved in that domain")
        found[33] = 1
    else:
        if found is not None:
            raise ValueError("key already present; revoked keys cannot be silently reapproved")
        records.append(bytearray(public + bytes([domain, 0]) + b"\0"*6 + struct.pack("<Q", args.epoch) + b"\0"*16))
    data = struct.pack("<8sIIQ8x", b"OSAPOL01", 1, len(records), args.epoch) + b"".join(records)
    atomic(args.policy, data, lambda p: checker(args, "policy", p), replace=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, required=True, help="built package-check executable")
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("pack", "index-pack"):
        p = sub.add_parser(name)
        p.add_argument("manifest", type=Path)
        p.add_argument("--key", type=Path, required=True, help="raw private seed OUTSIDE repository")
        p.add_argument("--output", type=Path, required=True)
        if name == "pack":
            p.add_argument("--executable", type=Path, help="override the source ELF path; never stored in package")
    for name in ("inspect", "verify", "index-inspect", "index-verify"):
        p = sub.add_parser(name)
        p.add_argument("file", type=Path)
        if name.endswith("verify"):
            p.add_argument("--policy", type=Path, required=True)
            if name == "verify":
                p.add_argument("--release-floor", type=int, default=0)
                p.add_argument("--rollback-digest", default="-")
            else:
                p.add_argument("--now", type=int, required=True, help="trusted UTC seconds; never taken from index")
                p.add_argument("--repository", required=True)
                p.add_argument("--sequence-floor", type=int, required=True)
                p.add_argument("--checkpoint", required=True, help="persisted digest; all zero only for sequence 0")
                p.add_argument("--package", type=Path)
    for name in ("trust-add", "trust-revoke"):
        p = sub.add_parser(name)
        p.add_argument("--policy", type=Path, required=True)
        p.add_argument("--public-key", type=Path, required=True)
        p.add_argument("--fingerprint", required=True, help="explicit SHA-256 approval of the public key")
        p.add_argument("--domain", choices=DOMAINS, required=True)
        p.add_argument("--epoch", type=int, required=True)
    args = parser.parse_args()
    try:
        if args.command in ("pack", "index-pack"):
            config = load(args.manifest)
            key = signing_key(args.key)
            if args.command == "pack":
                if args.executable:
                    config["package"]["executable"] = str(args.executable.resolve())
                data = pack(config, args.manifest.parent, key)
                check = "inspect"
            else:
                data = pack_index(args, config, args.manifest.parent, key)
                check = "inspect-index"
            atomic(args.output, data, lambda p: checker(args, check, p))
            print(f"created={args.output}\nsha256={hashlib.sha256(data).hexdigest()}")
        elif args.command.startswith("trust-"):
            # Serialize policy editors so a concurrent approval cannot overwrite
            # a revocation read from the previous epoch. A stale lock is a refusal.
            lock = args.policy.with_name(args.policy.name + ".lock")
            lockfile = lock.open("x")
            try:
                with lockfile:
                    trust(args)
            finally:
                lock.unlink()
            print("policy updated; no runtime installation or execution authorized")
        else:
            command = args.command.replace("index-", "") + ("-index" if args.command.startswith("index-") else "")
            extra = []
            if args.command == "verify":
                extra = [args.policy, args.release_floor, args.rollback_digest]
            elif args.command == "index-verify":
                extra = [args.policy, args.now, args.repository, args.sequence_floor, args.checkpoint]
                if args.package:
                    extra.append(args.package)
            print(checker(args, command, args.file, *extra), end="")
    except (ValueError, OSError, KeyError, TypeError, struct.error) as error:
        print(f"refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
