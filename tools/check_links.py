#!/usr/bin/env python3
"""Check that every relative Markdown link in docs/ and README.md points to an existing file.

Usage:  python3 tools/check_links.py [paths...]   (default: docs/ and README.md)
Exit status: 0 when every link resolves, 1 otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINK_RE = re.compile(r"\]\(([^)\s]+)\)")
CODE_FENCE = "```"


def links(path: Path):
    in_code = False
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if line.lstrip().startswith(CODE_FENCE):
            in_code = not in_code
            continue
        if in_code:
            continue
        for target in LINK_RE.findall(re.sub(r"`[^`]*`", "", line)):
            if re.match(r"^[a-z]+:", target) or target.startswith("#"):
                continue
            yield lineno, target


def main(argv: list[str]) -> int:
    targets = [Path(a).resolve() for a in argv] or [ROOT / "docs", ROOT / "README.md"]
    files: list[Path] = []
    for t in targets:
        files.extend(sorted(t.rglob("*.md")) if t.is_dir() else [t])
    files = [f for f in files if "plans" not in f.parts]
    broken = 0
    for f in files:
        for lineno, target in links(f):
            if not (f.parent / target.split("#", 1)[0]).exists():
                print(f"BROKEN {f.relative_to(ROOT)}:{lineno}: {target}")
                broken += 1
    print(f"{len(files)} files, {broken} broken links")
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
