#!/usr/bin/env python3
import datetime
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parent.parent
paths = [root / "Cargo.toml", root / "Cargo.lock", root / "rust-toolchain.toml", root / "design/themes.json", root / "design/tui-themes.json"]
paths.extend(p for p in (root / "crates").rglob("*") if p.is_file())
digest = hashlib.sha256()
for path in sorted(paths):
    digest.update(str(path.relative_to(root)).encode() + b"\0" + path.read_bytes())
info = {"version": "0.1.0", "source_sha256": digest.hexdigest(), "architecture": platform.machine(), "libc": platform.libc_ver(), "built_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "os_release": pathlib.Path("/etc/os-release").read_text(), "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(), "cargo": subprocess.check_output(["cargo", "--version"], text=True).strip(), "validation": "See docs/VALIDATION.md; building a package does not validate installation or runtime compatibility."}
info["packaging_revision"] = int((root / "packaging/revision").read_text().strip())
packaging_digest = hashlib.sha256()
for path in sorted(p for p in (root / "packaging").rglob("*") if p.is_file()):
    packaging_digest.update(str(path.relative_to(root)).encode() + b"\0" + path.read_bytes())
info["packaging_sha256"] = packaging_digest.hexdigest()
pathlib.Path(sys.argv[1]).write_text(json.dumps(info, indent=2) + "\n")
