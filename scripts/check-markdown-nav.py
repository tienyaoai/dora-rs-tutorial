#!/usr/bin/env python3
"""Verify relative file links and GitHub-style heading anchors in markdown."""
from __future__ import annotations

import re
import sys
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def collect_headings(path: Path) -> dict[str, str]:
    slugs: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^(#{1,6})\s+(.+)$", line)
        if not m:
            continue
        title = m.group(2).strip()
        slug = github_heading_slug(title)
        slugs[slug] = title
    return slugs


def github_heading_slug(text: str) -> str:
    """Approximate GitHub heading anchor for GFM (Unicode headings)."""
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = text.strip()
    out: list[str] = []
    for ch in text:
        if ch in " \t":
            out.append(" ")
        elif ch == "-":
            out.append("-")
        elif ch.isascii() and ch.isalnum():
            out.append(ch.lower())
        elif unicodedata.category(ch).startswith("L"):
            out.append(ch)
        # drop punctuation (colon, arrows, parentheses, slashes, etc.)
    slug = "".join(out)
    slug = re.sub(r"\s+", "-", slug.strip())
    slug = re.sub(r"-+", "-", slug)
    return slug


def main() -> int:
    md_files = sorted(ROOT.rglob("*.md"))
    if ROOT.name == "scripts":
        md_files = sorted(ROOT.parent.rglob("*.md"))
    md_files = [p for p in md_files if ".git" not in p.parts]

    link_re = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
    errors: list[str] = []

    for path in md_files:
        text = path.read_text(encoding="utf-8")
        headings = collect_headings(path)
        for m in link_re.finditer(text):
            raw = m.group(1)
            if raw.startswith("http") or raw.startswith("mailto:"):
                continue
            target, _, frag = raw.partition("#")
            if target:
                dest = (path.parent / target).resolve()
                if not dest.exists():
                    errors.append(f"{path.relative_to(ROOT)}: missing file {target}")
            if frag:
                anchor_file = path
                if target:
                    anchor_file = (path.parent / target).resolve()
                anchor_headings = collect_headings(anchor_file)
                if frag not in anchor_headings:
                    errors.append(
                        f"{path.relative_to(ROOT.parent if ROOT.name == 'scripts' else ROOT)}: "
                        f"bad anchor #{frag} in {anchor_file.name}"
                    )

    if errors:
        for e in errors:
            print("ERROR", e)
        return 1
    print(f"OK: {len(md_files)} markdown files, links and anchors checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
