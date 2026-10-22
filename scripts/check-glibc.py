#!/usr/bin/env python3
"""Reject Linux release binaries that require a newer glibc than the baseline."""

import argparse
import re
import subprocess
from pathlib import Path


def check_glibc(binary, maximum):
    symbols = subprocess.check_output(
        ["readelf", "--dyn-syms", "--wide", binary], text=True
    )
    versions = {
        tuple(map(int, version.split(".")))
        for line in symbols.splitlines()
        if " UND " in line
        for version in re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", line)
    }
    if not versions:
        raise ValueError(f"No glibc requirements found in {binary}")
    required = ".".join(map(str, max(versions)))
    if max(versions) > tuple(map(int, maximum.split("."))):
        raise ValueError(f"{binary} requires glibc {required}; maximum is {maximum}")
    print(f"{binary}: glibc {required} <= {maximum}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-version", required=True)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    try:
        check_glibc(args.binary, args.max_version)
    except ValueError as error:
        raise SystemExit(str(error)) from error
