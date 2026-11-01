#!/usr/bin/env python3
"""Collect license texts/notices of each client's resolved dependency closure."""
import json
import pathlib
import shutil
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", "x86_64-unknown-linux-gnu"], cwd=root))
packages = {p["id"]: p for p in metadata["packages"]}
nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
for binary, crate in [("harbor", "harbor-desktop"), ("harbor-tui", "harbor-tui")]:
    first = next(p["id"] for p in packages.values() if p["name"] == crate)
    seen, pending = set(), [first]
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        pending.extend(nodes[current]["dependencies"])
    output = root / "target" / "licenses" / binary
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    index = []
    for package_id in sorted(seen):
        package = packages[package_id]
        if package["source"] is None:
            continue
        directory = pathlib.Path(package["manifest_path"]).parent
        destination = output / (package["name"] + "-" + package["version"])
        texts = [p for p in directory.rglob("*") if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))]
        if package["license_file"]:
            texts.append(directory / package["license_file"])
        for path in set(texts):
            target = destination / path.relative_to(directory)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
        index.append({"name": package["name"], "version": package["version"], "license": package["license"], "license_texts": len(set(texts))})
    (output / "index.json").write_text(json.dumps(index, indent=2) + "\n")
    rustc = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    rust_licenses = pathlib.Path(rustc) / "share/doc/rust/licenses"
    if rust_licenses.is_dir():
        shutil.copytree(rust_licenses, output / "rust-runtime", dirs_exist_ok=True)
    print(f"Collected {len(index)} dependency license entries for {binary}")
