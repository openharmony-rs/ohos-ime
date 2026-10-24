#!/usr/bin/env python3
"""Installs the ohpm packages of the project without ohpm.

ohpm ships only with the HarmonyOS command line tools. This installs the registry packages that
`oh-package-lock.json5` pins into `oh_modules`, and links the local (`file:`) dependencies of the
modules into their `oh_modules`, as ohpm does. The JSON5 files are read as JSON, so they must not
contain comments.
"""

import argparse
import base64
import hashlib
import io
import json
import os
import sys
import tarfile
import urllib.request
from pathlib import Path


def read_json5(path: Path) -> dict:
    return json.loads(path.read_text())


def install_package(project: Path, package: dict) -> None:
    target = project / "oh_modules" / package["name"]
    if target.exists():
        installed = read_json5(target / "oh-package.json5")["version"]
        if installed != package["version"]:
            raise SystemExit(f"{target} holds version {installed} instead of {package['version']}, remove it")
        return
    with urllib.request.urlopen(package["resolved"]) as response:
        har = response.read()
    algorithm, digest = package["integrity"].split("-", 1)
    if hashlib.new(algorithm, har).digest() != base64.b64decode(digest):
        raise SystemExit(f"{package['resolved']} does not match {package['integrity']}")
    with tarfile.open(fileobj=io.BytesIO(har)) as archive:
        members = []
        for member in archive.getmembers():
            # The files of a HAR are in a `package` directory.
            if member.name.startswith("package/"):
                member.name = member.name.removeprefix("package/")
                members.append(member)
        archive.extractall(target, members=members, filter="data")
    print(f"Installed {package['name']}@{package['version']}")


def link_local_dependencies(module: Path) -> None:
    dependencies = read_json5(module / "oh-package.json5").get("dependencies", {})
    for name, specifier in dependencies.items():
        if not specifier.startswith("file:"):
            continue
        link = module / "oh_modules" / name
        if link.is_symlink() or link.exists():
            continue
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(os.path.relpath(module / specifier.removeprefix("file:"), link.parent))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path, nargs="?", default=Path("."), help="the project (default: .)")
    args = parser.parse_args()

    for package in read_json5(args.project / "oh-package-lock.json5")["packages"].values():
        if package.get("registryType") == "ohpm":
            install_package(args.project, package)
    modules = [args.project] + [
        args.project / module["srcPath"] for module in read_json5(args.project / "build-profile.json5")["modules"]
    ]
    for module in modules:
        link_local_dependencies(module)
    return 0


if __name__ == "__main__":
    sys.exit(main())
