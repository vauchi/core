#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Retire the legacy hardware module in the release's cloned Swift package."""

import re
import shutil
import sys
import tempfile
from pathlib import Path


def retire(package: Path) -> None:
    manifest = package / "Package.swift"
    sources = package / "Sources"
    hardware = sources / "VauchiHardware"
    if any(path.is_symlink() for path in (package, manifest, sources, hardware)):
        raise ValueError("refusing a symlink in the Swift package cleanup paths")

    original = manifest.read_text()
    updated = re.sub(
        r"(?ms)^let vauchiHardware(?:Product|Target):[^\n]+\n.*?^\)\n",
        "",
        original,
    )
    updated = re.sub(
        r"(?m)^/// Shared Apple-platform hardware services[^\n]*\n(?:///[^\n]*\n)*",
        "",
        updated,
    )
    updated = updated.replace("vauchiHardwareProduct, ", "")
    updated = updated.replace("vauchiHardwareTarget, ", "")
    if "VauchiHardware" in updated or "vauchiHardware" in updated:
        raise ValueError("unsupported VauchiHardware manifest layout")

    if not hardware.exists():
        if updated != original:
            manifest.write_text(updated)
        return

    with tempfile.TemporaryDirectory(prefix="vauchi-retired-swift-hardware-") as temporary:
        backup = Path(temporary) / "VauchiHardware"
        shutil.copytree(hardware, backup, symlinks=True)
        try:
            if updated != original:
                manifest.write_text(updated)
            shutil.rmtree(hardware)
        except OSError:
            manifest.write_text(original)
            shutil.copytree(backup, hardware, dirs_exist_ok=True, symlinks=True)
            raise


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: retire-swift-hardware.py <cloned-package>")
    try:
        retire(Path(sys.argv[1]))
    except (OSError, ValueError) as error:
        sys.exit(f"Swift hardware retirement failed: {error}")
