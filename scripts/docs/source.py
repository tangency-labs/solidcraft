"""Read the existing English mdBook sources without relocating them."""

import re
from pathlib import Path

ENGLISH = Path("docs/book/src")
INCLUDE = re.compile(r"\{\{#include\s+([^}]+)\}\}")


def english_page(path, root, reader=None):
    """Expand shared source includes for editable translations and track dependencies."""
    root = root.resolve()
    dependencies = set()

    def expand(current, seen=()):
        current = current.resolve()
        dependencies.add(current.relative_to(root).as_posix())
        if current in seen:
            raise ValueError("Recursive documentation include: " + str(current))
        text = reader(current) if reader else current.read_text(encoding="utf-8")

        def include(match):
            filename, separator, anchor = match[1].strip().partition(":")
            content = expand(current.parent / filename, seen + (current,))
            if separator:
                if re.fullmatch(r"\d*(?::\d*)?", anchor):
                    start, _, end = anchor.partition(":")
                    first = int(start) - 1 if start else 0
                    last = int(end) if end else int(start) if start and ":" not in anchor else None
                    content = "\n".join(content.splitlines()[first:last])
                else:
                    begin, end = "<!-- ANCHOR: " + anchor + " -->", "<!-- ANCHOR_END: " + anchor + " -->"
                    if begin not in content or end not in content:
                        raise ValueError("Missing documentation include anchor: " + anchor)
                    content = content.split(begin, 1)[1].split(end, 1)[0].strip("\n")
            return content

        return INCLUDE.sub(include, text)

    return expand(path).rstrip() + "\n", dependencies
