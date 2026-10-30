#!/usr/bin/env python3
"""Refuse stale release text before publication; never rewrite old releases."""
import argparse
from pathlib import Path
import re


def validate(notes, changelog, tag):
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("Expected a numeric release tag")
    first = notes.lstrip().splitlines()[0] if notes.strip() else ""
    match = re.match(r"RUWAC (v[0-9]+\.[0-9]+\.[0-9]+)(?=\s|$)", first)
    if match is None or match[1] != tag:
        raise ValueError("Release notes must start with RUWAC " + tag)
    sections = re.findall(r"^## " + re.escape(tag) + r"(?:\s[^\n]*)?$", changelog, re.M)
    if len(sections) != 1:
        raise ValueError("Release tag must have exactly one changelog section")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        validate((args.root / "RELEASE-NOTES.md").read_text(),
                 (args.root / "CHANGELOG.md").read_text(), args.tag)
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")
    print("Release notes match " + args.tag)


if __name__ == "__main__":
    main()
