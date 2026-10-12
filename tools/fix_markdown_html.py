#!/usr/bin/env python3
"""Make chapter Markdown render the same on GitHub and in mdBook.

Fixes, all outside fenced code blocks:
1. `<details>` blocks: put a blank line after `<summary>…</summary>` and before
   `</details>`, so the content inside is parsed as Markdown (code spans,
   footnote references, emphasis) instead of raw HTML. A `<details>` written at
   the end of a list item (`1. Question? <details>…</details>`) is moved onto
   its own indented lines under that item.
2. A `<` that would open a bogus HTML tag (`Vec<T>`, `F<Sub>`, `<'a>`) outside an
   inline code span is escaped as `\\<`. Real tags (`<details>`, `<sub>`, …) and
   autolinks (`<https://…>`) are left alone. The citation verifier ignores the
   backslash.
3. Inside `<summary>` (raw HTML, where Markdown is not parsed) `code` becomes
   <code>…</code> and a bogus `<` becomes `&lt;`.

Usage: python3 tools/fix_markdown_html.py [--check] [files or dirs...]  (default: docs/)
With --check, report files that would change and exit 1 if any.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTML_TAGS = {
    "a", "b", "br", "code", "details", "div", "em", "hr", "i", "img", "kbd", "li",
    "mark", "ol", "p", "pre", "small", "span", "strong", "sub", "summary", "sup",
    "table", "tbody", "td", "th", "thead", "tr", "ul",
}
FENCE = re.compile(r"^\s*(```|~~~)")
LIST_INLINE = re.compile(
    r"^(\s*)(\d+\.|[-*])(\s+)(.*?)\s*(<details>\s*<summary>.*?</summary>)(.*?)</details>\s*$"
)
ONE_LINE = re.compile(r"^(\s*)(<details>\s*<summary>.*?</summary>)(.+?)</details>\s*$")
OPEN_WITH_BODY = re.compile(r"^(\s*)(<details>\s*<summary>.*?</summary>)(.+)$")
SUMMARY = re.compile(r"(<summary>)(.*?)(</summary>)")
SUMMARY_END = re.compile(r"</summary>\s*$")
CLOSE = re.compile(r"^\s*</details>\s*$")
TAG_START = re.compile(r"(/?)([A-Za-z_'][A-Za-z0-9_-]*)")
SCHEME = re.compile(r"[a-z][a-z0-9+.-]*:(?!:)")  # `<https:` yes, `<Self::Item>` no


def is_real_tag(rest: str) -> bool:
    """`rest` is the text right after a `<`."""
    if SCHEME.match(rest):
        return True  # autolink <https://…>
    m = TAG_START.match(rest)
    if not m:
        return True  # `<-`, `<=`, `<<`: not a tag opener, leave alone
    name = m.group(2)
    after = rest[m.end():m.end() + 1]
    if name not in HTML_TAGS or after not in ("", ">", " ", "/"):
        return False
    return not (name == "a" and after == ">")  # a bare <a> is a placeholder, not a link


def escape_text(seg: str, esc: str = "\\<") -> str:
    out = []
    for i, ch in enumerate(seg):
        if ch == "<" and (i == 0 or seg[i - 1] != "\\") and not is_real_tag(seg[i + 1:]):
            out.append(esc)
        else:
            out.append(ch)
    return "".join(out)


def escape_line(line: str) -> str:
    """Escape bogus tags in text, leaving inline code spans untouched."""
    out, i = [], 0
    while i < len(line):
        if line[i] == "`":
            j = i
            while j < len(line) and line[j] == "`":
                j += 1
            ticks = line[i:j]
            k = line.find(ticks, j)
            if k == -1:
                out.append(escape_text(line[i:]))
                break
            out.append(line[i:k + len(ticks)])
            i = k + len(ticks)
        else:
            j = line.find("`", i)
            j = len(line) if j == -1 else j
            out.append(escape_text(line[i:j]))
            i = j
    return "".join(out)


QUOTED_LINK = re.compile(r"(?<!\\)\[([^\]\\]+)\]\(([^)\s]+)\)")


def footnote_quote(line: str) -> str:
    """In a footnote definition, the quoted source text may contain Markdown links
    copied from the source (`[result](#Section-5)`); escape them so they render as
    text instead of becoming broken links. The verifier ignores the backslashes."""
    cut = line.rfind(' — "')
    if not line.startswith("[^") or cut == -1:
        return line
    head, quote = line[:cut], line[cut:]
    return head + QUOTED_LINK.sub(lambda m: f"\\[{m.group(1)}\\]\\({m.group(2)}\\)", quote)


def html_summary(line: str) -> str:
    def conv(m: re.Match) -> str:
        res = []
        for p in re.split(r"(`[^`]+`)", m.group(2)):
            if len(p) > 1 and p.startswith("`") and p.endswith("`"):
                inner = p[1:-1].replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
                res.append(f"<code>{inner}</code>")
            else:
                res.append(escape_text(p, "&lt;"))
        return m.group(1) + "".join(res) + m.group(3)

    return SUMMARY.sub(conv, line)


def fix(text: str) -> str:
    out: list[str] = []
    in_fence = False
    for line in text.split("\n"):
        if FENCE.match(line):
            in_fence = not in_fence
            out.append(line)
            continue
        if in_fence:
            out.append(line)
            continue
        # Blank line after a bare `<details><summary>…</summary>` line, before `</details>`,
        # and after `</details>` (an HTML block runs until a blank line).
        prev = out[-1] if out else ""
        if line.strip() and SUMMARY_END.search(prev) and "<summary>" in prev:
            out.append("")
        elif CLOSE.match(line) and prev.strip():
            out.append("")
        elif line.strip() and CLOSE.match(prev):
            out.append("")

        m = LIST_INLINE.match(line)
        if m:
            ind, marker, sp, q, head, body = m.groups()
            sub = ind + " " * (len(marker) + len(sp))
            out += [ind + marker + sp + escape_line(q), "", sub + html_summary(head), "",
                    sub + escape_line(body.strip()), "", sub + "</details>"]
            continue
        m = ONE_LINE.match(line)
        if m:
            ind, head, body = m.group(1), m.group(2), m.group(3).strip()
            out += [ind + html_summary(head), "", ind + escape_line(body), "", ind + "</details>"]
            continue
        m = OPEN_WITH_BODY.match(line)
        if m and m.group(3).strip():
            ind, head, body = m.group(1), m.group(2), m.group(3).strip()
            out += [ind + html_summary(head), "", ind + escape_line(body)]
            continue
        stripped = line.lstrip()
        if stripped.startswith(("<details", "<summary")):
            out.append(html_summary(line))
        elif stripped.startswith("</details"):
            out.append(line)
        else:
            out.append(footnote_quote(escape_line(line)))
    return "\n".join(out)


def targets(args: list[str]):
    paths = [Path(a) for a in args] or [ROOT / "docs"]
    for p in paths:
        for f in (sorted(p.rglob("*.md")) if p.is_dir() else [p]):
            if "plans" not in f.parts:
                yield f


def main(argv: list[str]) -> int:
    check = "--check" in argv
    changed = []
    for f in targets([a for a in argv if a != "--check"]):
        old = f.read_text(encoding="utf-8")
        new = fix(old)
        if new != old:
            changed.append(f)
            if not check:
                f.write_text(new, encoding="utf-8")
    for f in changed:
        print(("would fix " if check else "fixed ") + str(f.resolve().relative_to(ROOT.resolve())))
    print(f"{len(changed)} files {'need fixing' if check else 'fixed'}")
    return 1 if (check and changed) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
