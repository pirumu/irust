#!/usr/bin/env python3
"""mdBook preprocessor: keep TeX inside $...$ / $$...$$ intact through Markdown.

Markdown strips backslash escapes (\\{ -> {, \\, -> ,) and reads _ * [ < as
emphasis/link/HTML syntax. MathJax runs on the rendered HTML, so every such
character inside a math span is escaped here so Markdown emits it verbatim.
"""
import json
import re
import sys

ASCII_PUNCT = set("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~")
MARKDOWN_ACTIVE = set("_*[]<>`~|")
FENCE = re.compile(r"^\s*(```|~~~)")
MATH = re.compile(r"\$\$(.+?)\$\$|\$([^$\n]+?)\$", re.S)
INLINE_CODE = re.compile(r"(`+)(.+?)\1", re.S)


def escape_tex(tex):
    out = []
    i = 0
    while i < len(tex):
        c = tex[i]
        if c == "\\" and i + 1 < len(tex) and tex[i + 1] in ASCII_PUNCT:
            out.append("\\\\\\" + tex[i + 1])
            i += 2
            continue
        out.append("\\" + c if c in MARKDOWN_ACTIVE else c)
        i += 1
    return "".join(out)


def protect_math(segment):
    def repl(m):
        if m.group(1) is not None:
            return "$$" + escape_tex(m.group(1)) + "$$"
        return "$" + escape_tex(m.group(2)) + "$"

    pieces = []
    last = 0
    for code in INLINE_CODE.finditer(segment):
        pieces.append(MATH.sub(repl, segment[last:code.start()]))
        pieces.append(code.group(0))
        last = code.end()
    pieces.append(MATH.sub(repl, segment[last:]))
    return "".join(pieces)


def transform(content):
    out, prose, in_fence = [], [], False

    def flush():
        if prose:
            out.append(protect_math("".join(prose)))
            prose.clear()

    for line in content.splitlines(keepends=True):
        if FENCE.match(line):
            if not in_fence:
                flush()
            in_fence = not in_fence
            out.append(line)
        elif in_fence or line.startswith("[^"):
            flush()
            out.append(line)
        else:
            prose.append(line)
    flush()
    return "".join(out)


def walk(node):
    if isinstance(node, dict):
        if "content" in node and "sub_items" in node and isinstance(node["content"], str):
            node["content"] = transform(node["content"])
        for value in node.values():
            walk(value)
    elif isinstance(node, list):
        for value in node:
            walk(value)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "supports":
        sys.exit(0)
    _context, book = json.load(sys.stdin)
    walk(book)
    json.dump(book, sys.stdout)
