"""Build documentation languages, filling untranslated pages from English."""

import argparse
import hashlib
import json
import os
import posixpath
import re
import shutil
import subprocess
from pathlib import Path, PurePosixPath

from source import ENGLISH, english_page

INCLUDE = re.compile(r"\{\{#include\s+([^}]+)\}\}")


def include_paths(text, original, staged):
    """Keep mdBook includes, including named anchors, relative to their real source."""
    def replace(match):
        filename, separator, anchor = match.group(1).strip().partition(":")
        source = (original.parent / filename).resolve()
        relative = os.path.relpath(source, staged.parent).replace(os.sep, "/")
        return "{{#include " + relative + (separator + anchor if separator else "") + "}}"
    return INCLUDE.sub(replace, text)


def language_links(relative, current, languages):
    page = PurePosixPath(relative).with_suffix(".html")
    here = page if current == "en" else PurePosixPath(current) / page
    links = []
    for language in languages:
        code, name = language["id"], language["name"]
        destination = page if code == "en" else PurePosixPath(code) / page
        href = posixpath.relpath(str(destination), str(here.parent))
        links.append("**" + name + "**" if code == current else "[" + name + "](" + href + ")")
    return " · ".join(links)


def summary_text(english, translated, headings):
    """Keep the complete English chapter list; translate labels by stable page path."""
    labels = {target: label for label, target in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", translated)}
    text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", lambda match: "[" + labels.get(match[2], match[1]) + "](" + match[2] + ")", english)
    return re.sub(r"^(#+) (.+)$", lambda match: match[1] + " " + headings.get(match[2], match[2]), text, flags=re.MULTILINE)


def command_reference(text, root, code):
    """Render live registry entries with localized labels and untouched parameter syntax."""
    marker = "{{#command-reference}}"
    if marker not in text:
        return text
    source = (root / ENGLISH / "reference/commands.md").read_text(encoding="utf-8")
    english = json.loads((root / "assets/locales/en.json").read_text(encoding="utf-8"))
    localized = json.loads((root / "assets/locales" / (code + ".json")).read_text(encoding="utf-8"))
    labels = {value: localized[key] for key, value in english.items() if key in localized}
    # Only the generated introduction is replaced; command order, IDs, shortcuts,
    # parameter documentation, code examples and links remain byte-for-byte intact.
    start = re.search(r"(?m)^(?:## |\- `)", source)
    if start is None:
        raise ValueError("The generated command reference contains no command sections")
    catalogue = source[start.start():]
    def translate(match):
        label = match[2]
        translated = labels.get(label, label)
        name = label if translated == label else label + " — " + translated
        return match[1] + name + match[3]
    lines = []
    fence = None
    for line in catalogue.splitlines(keepends=True):
        delimiter = re.match(r"^\s*(`{3,}|~{3,})", line)
        if delimiter:
            token = delimiter[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            lines.append(line)
        elif fence is not None:
            lines.append(line)
        else:
            lines.append(re.sub(r"^(- `[^`]+` \*\*)([^\n]*?)(\*\*)", translate, line))
    catalogue = "".join(lines)
    return text.replace(marker, catalogue.rstrip())


def prepare(root, language, languages):
    code = language["id"]
    english = root / ENGLISH
    translated = root / "docs" / code
    stage = root / "target/docs-i18n" / code
    # Check the final absolute path before deleting disposable build output.
    stage.resolve().relative_to((root / "target/docs-i18n").resolve())
    if stage.exists():
        shutil.rmtree(stage)
    staged_source = stage
    shutil.copytree(english, staged_source)
    chapters = []
    manifest = translated / ".translation.json"
    tracked = json.loads(manifest.read_text(encoding="utf-8"))["pages"] if manifest.exists() else {}
    for original in sorted(english.rglob("*.md")):
        relative = original.relative_to(english)
        if relative.as_posix() == "README.md":
            continue
        english_text, _ = english_page(original, root)
        candidate = translated / relative
        source = candidate if code != "en" and candidate.is_file() and candidate.read_text(encoding="utf-8").strip() else original
        if code != "en" and source == candidate and tracked.get(relative.as_posix(), {}).get("copy"):
            copied = hashlib.sha256(candidate.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
            if copied == tracked[relative.as_posix()]["copy"]:
                source = original
        source_text = english_text if source == original else english_page(source, root)[0]
        text = command_reference(source_text, root, code)
        text = include_paths(text, source, staged_source / relative)
        fallback = code != "en" and (source == original or source_text == english_text)
        if relative.name == "SUMMARY.md":
            text = summary_text(english_text, text, language.get("headings", {}))
        else:
            if code != "en":
                # The web app is shared at /app/, not duplicated under each language.
                text = re.sub(r"(\]\()((?:\.\./)*app/[^)]*)(\))", lambda match: match[1] + "../" + match[2] + match[3], text)
            navigation = language_links(relative.as_posix(), code, languages)
            if fallback:
                notice = (
                    "Denne side er endnu ikke oversat til dansk. Den engelske original vises nedenfor."
                    if code == "da" else
                    "This page has not yet been translated into " + language["name"] + ". The English original is shown below."
                )
                text = navigation + "\n\n> " + notice + '\n\n<div lang="en">\n\n' + text + "\n\n</div>\n"
            else:
                text = navigation + "\n\n" + text
            chapters.append({"page": relative.as_posix(), "language": "en" if fallback else code, "fallback": fallback})
        (staged_source / relative).write_text(text, encoding="utf-8", newline="\n")
    config = (root / "docs/book/book.toml").read_text(encoding="utf-8")
    config = config.replace('src = "src"', 'src = "."')
    any_translation = any(not chapter["fallback"] for chapter in chapters) if code != "en" else False
    config = config.replace('language = "en"', 'language = ' + json.dumps(code if any_translation else "en"))
    config = config.replace('site-url = "/"', 'site-url = ' + json.dumps("/" if code == "en" else "/" + code + "/"))
    # English keeps its edit button; a locale-wide URL would target missing fallback files.
    if code == "en":
        config = config.replace("/edit/main/docs/book/{path}", "/edit/main/docs/book/src/{path}")
    else:
        config = "\n".join(line for line in config.splitlines() if not line.startswith("edit-url-template")) + "\n"
    (stage / "book.toml").write_text(config, encoding="utf-8", newline="\n")
    shutil.copytree(root / "docs/book/theme", stage / "theme")
    return stage, chapters


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--mdbook", default="mdbook")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    languages = json.loads((root / "docs/languages.json").read_text(encoding="utf-8"))
    ids = [language["id"] for language in languages]
    if not ids or ids[0] != "en" or len(set(ids)) != len(ids) or any(not re.fullmatch(r"[a-z]{2}(?:-[A-Za-z0-9]+)?", code) for code in ids):
        parser.error("languages.json must list English first and unique language IDs")
    report = {}
    for language in languages:
        stage, chapters = prepare(root, language, languages)
        report[language["id"]] = chapters
        if not args.prepare_only:
            output = root / "target/book"
            if language["id"] != "en":
                output = output / language["id"]
            subprocess.run([args.mdbook, "build", str(stage), "--dest-dir", str(output)], check=True)
        print(language["id"] + ": " + str(sum(page["fallback"] for page in chapters)) + " English fallback pages")
    (root / "target/docs-i18n/coverage.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
