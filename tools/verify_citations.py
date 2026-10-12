#!/usr/bin/env python3
"""Check that every footnote quote in the curriculum really appears in its source.

Footnote format (one line each, see README.md):

    [^id]: Short title, section — <https://example.org/page> — "verbatim quote"

For each footnote the script downloads the URL (cached under .cache/sources/),
extracts its text (HTML via the stdlib parser, PDF via `pdftotext`, anything else
such as raw source files as-is), normalizes
both sides to lowercase alphanumeric words and checks the quote is a contiguous
word sequence of the source. It also checks that every `[^id]` used in the text
is defined and every definition is used.

Usage:  python3 tools/verify_citations.py [paths...]   (default: docs/)
Exit status: 0 when every check passes, 1 otherwise.
"""

from __future__ import annotations

import hashlib
import html
import re
import subprocess
import sys
import unicodedata
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / ".cache" / "sources"
USER_AGENT = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/126.0 Safari/537.36"
)
MIN_QUOTE_WORDS = 5
MAX_QUOTE_WORDS = 80

DEF_RE = re.compile(r"^\[\^([A-Za-z0-9_-]+)\]:\s*(.*)$")
REF_RE = re.compile(r"\[\^([A-Za-z0-9_-]+)\]")
INLINE_CODE_RE = re.compile(r"`[^`]*`")
BODY_RE = re.compile(r"<(?P<url>https?://[^>\s]+)>\s+—\s+\"(?P<quote>.+)\"\s*$")


@dataclass
class Footnote:
    file: Path
    line: int
    fid: str
    url: str
    quote: str


class _TextExtractor(HTMLParser):
    SKIP = {"script", "style", "noscript", "svg"}
    BLOCK = {"p", "div", "li", "br", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "dt", "dd", "td", "th"}

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []
        self._skip = 0

    def handle_starttag(self, tag, attrs):
        if tag in self.SKIP:
            self._skip += 1
        elif tag in self.BLOCK:
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in self.SKIP and self._skip:
            self._skip -= 1
        elif tag in self.BLOCK:
            self.parts.append("\n")

    def handle_data(self, data):
        if not self._skip:
            self.parts.append(data)


def normalize(text: str) -> str:
    text = unicodedata.normalize("NFKC", text).lower()
    text = text.replace("\u00ad", "")  # soft hyphen
    # Treat apostrophes as part of words so "don't" and "don’t" compare equal.
    text = re.sub(r"[’'`]", "", text)
    return " ".join(re.findall(r"[a-z0-9]+", text))


def fetch(url: str) -> tuple[str | None, str | None]:
    """Return (text, error). Text is cached on disk by URL hash."""
    CACHE.mkdir(parents=True, exist_ok=True)
    key = hashlib.sha256(url.encode()).hexdigest()[:24]
    cached = CACHE / f"{key}.txt"
    if cached.exists():
        return cached.read_text(encoding="utf-8"), None
    page_url = url.split("#", 1)[0]
    req = urllib.request.Request(page_url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            body = resp.read()
            ctype = resp.headers.get("Content-Type", "")
    except Exception as exc:  # network errors are reported, never ignored
        return None, f"fetch failed: {exc}"
    if "pdf" in ctype or page_url.lower().endswith(".pdf") or body[:5] == b"%PDF-":
        pdf = CACHE / f"{key}.pdf"
        pdf.write_bytes(body)
        proc = subprocess.run(["pdftotext", "-enc", "UTF-8", str(pdf), "-"], capture_output=True)
        if proc.returncode != 0:
            return None, f"pdftotext failed: {proc.stderr.decode(errors='replace').strip()}"
        raw = proc.stdout.decode("utf-8", errors="replace")
        # Rejoin words split by end-of-line hyphenation ("vir-\ntual").
        text = raw + "\n" + re.sub(r"(\w)-\s*\n\s*(\w)", r"\1\2", raw)
    else:
        charset = re.search(r"charset=([\w-]+)", ctype)
        decoded = body.decode(charset.group(1) if charset else "utf-8", errors="replace")
        if "html" in ctype.lower():
            parser = _TextExtractor()
            parser.feed(decoded)
            text = html.unescape("".join(parser.parts))
        else:
            # Plain text (e.g. raw .rs sources): `<T>` is code, not a tag.
            text = decoded
    cached.write_text(text, encoding="utf-8")
    return text, None


def parse(path: Path) -> tuple[list[Footnote], list[str]]:
    notes: list[Footnote] = []
    errors: list[str] = []
    refs: set[str] = set()
    defined: dict[str, int] = {}
    in_code = False
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if line.lstrip().startswith("```"):
            in_code = not in_code
            continue
        if in_code:
            continue
        m = DEF_RE.match(line)
        if m:
            fid, rest = m.groups()
            if fid in defined:
                errors.append(f"{path}:{lineno}: footnote [^{fid}] defined twice (first at line {defined[fid]})")
            defined[fid] = lineno
            body = BODY_RE.search(rest)
            if not body:
                errors.append(f"{path}:{lineno}: footnote [^{fid}] lacks `<url> — \"quote\"`")
                continue
            notes.append(Footnote(path, lineno, fid, body["url"], body["quote"]))
            continue
        refs.update(REF_RE.findall(INLINE_CODE_RE.sub("", line)))
    for fid in sorted(refs - defined.keys()):
        errors.append(f"{path}: [^{fid}] is used but never defined")
    for fid in sorted(defined.keys() - refs):
        errors.append(f"{path}:{defined[fid]}: [^{fid}] is defined but never used")
    return notes, errors


def check(note: Footnote, sources: dict[str, tuple[str | None, str | None]]) -> str | None:
    words = normalize(note.quote).split()
    where = f"{note.file}:{note.line}: [^{note.fid}]"
    if len(words) < MIN_QUOTE_WORDS:
        return f"{where} quote has {len(words)} words, needs at least {MIN_QUOTE_WORDS}"
    if len(words) > MAX_QUOTE_WORDS:
        return f"{where} quote has {len(words)} words, at most {MAX_QUOTE_WORDS} allowed"
    text, err = sources[note.url]
    if err:
        return f"{where} {note.url}: {err}"
    if f" {' '.join(words)} " not in f" {normalize(text)} ":
        return f"{where} quote not found in {note.url}: \"{note.quote}\""
    return None


def main(argv: list[str]) -> int:
    targets = [Path(a) for a in argv] or [ROOT / "docs"]
    files: list[Path] = []
    for t in targets:
        files.extend(sorted(t.rglob("*.md")) if t.is_dir() else [t])
    files = [f for f in files if "plans" not in f.parts]

    notes: list[Footnote] = []
    errors: list[str] = []
    for f in files:
        n, e = parse(f)
        notes.extend(n)
        errors.extend(e)

    urls = sorted({n.url for n in notes})
    with ThreadPoolExecutor(max_workers=8) as pool:
        sources = dict(zip(urls, pool.map(fetch, urls)))
    errors.extend(e for e in (check(n, sources) for n in notes) if e)

    for e in errors:
        print(f"FAIL {e}")
    print(f"{len(files)} files, {len(notes)} footnotes, {len(urls)} sources, {len(errors)} problems")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
