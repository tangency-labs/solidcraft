"""Synchronize English documentation copies and report translation work."""

import argparse
import difflib
import hashlib
import json
import os
import re
import subprocess
from pathlib import Path

from source import ENGLISH, english_page

BEGIN = "<!-- docs-english-updates:start -->"
END = "<!-- docs-english-updates:end -->"


def markdown_blocks(text):
    """Split paragraphs without cutting fenced code examples at blank lines."""
    blocks, lines, fence = [], [], None
    for line in text.splitlines(keepends=True):
        delimiter = re.match(r"^\s*(`{3,}|~{3,})", line)
        if delimiter:
            token = delimiter[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
        if not line.strip() and fence is None:
            if lines:
                blocks.append("".join(lines).rstrip("\n"))
                lines = []
        else:
            lines.append(line)
    if lines:
        blocks.append("".join(lines).rstrip("\n"))
    return blocks


def english_updates(before, after):
    old, new = markdown_blocks(before), markdown_blocks(after)
    additions = []
    removed = False
    for operation, a, b, c, d in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes():
        if operation in ("insert", "replace"):
            # Include the nearest source heading so isolated paragraphs have context.
            heading = next((block for block in reversed(new[:c]) if re.match(r"^#{1,6} ", block)), None)
            if heading and (not additions or additions[-1] != heading):
                additions.append(heading)
            additions.extend(new[c:d])
        if operation in ("delete", "replace"):
            removed = True
    note = "The English original also removes or replaces text. Review the corresponding translated text.\n\n" if removed else ""
    return note + "\n\n".join(additions)


def update_summary(english, translated, baseline):
    labels = dict((target, label) for label, target in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", translated))
    headings = {}
    old = re.findall(r"^(#+ .+)$", baseline, re.M)
    local = re.findall(r"^(#+ .+)$", translated, re.M)
    if len(old) == len(local):
        headings = dict(zip(old, local))
    text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", lambda m: "[" + labels.get(m[2], m[1]) + "](" + m[2] + ")", english)
    return re.sub(r"^#+ .+$", lambda m: headings.get(m[0], m[0]), text, flags=re.M)


def insert_updates(target, before, after, previous, code):
    text = target.read_text(encoding="utf-8")
    match = re.search(re.escape(BEGIN) + r".*?" + re.escape(END), text, re.S)
    if match and hashlib.sha256(match[0].encode()).hexdigest() != previous.get("pending_copy"):
        # The translator has started editing this block. Never overwrite their work.
        return
    changes = english_updates(before, after)
    if not changes.strip():
        if match:
            target.write_text((text[:match.start()] + text[match.end():]).rstrip() + "\n", encoding="utf-8", newline="\n")
            previous.pop("pending_copy", None)
        return
    title = "Engelske ændringer, der mangler oversættelse" if code == "da" else "English updates awaiting translation"
    notice = (
        "Nedenfor står nye og ændrede afsnit fra den engelske original. Arbejd dem ind i oversættelsen, "
        "kontrollér den aktuelle engelske side, og fjern derefter hele denne markerede sektion."
        if code == "da" else
        "Integrate these new and changed English sections into the translation, check the current English page, "
        "then remove this entire marked section."
    )
    block = BEGIN + "\n\n## " + title + "\n\n> " + notice + "\n\n" + changes.rstrip() + "\n\n" + END
    text = text[:match.start()] + block + text[match.end():] if match else text.rstrip() + "\n\n" + block + "\n"
    target.write_text(text, encoding="utf-8", newline="\n")
    previous["pending_copy"] = hashlib.sha256(block.encode()).hexdigest()


def digest(path):
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def synchronize(root, review=None, revision=None):
    revision = revision or git(root, "rev-parse", "HEAD")
    languages = json.loads((root / "docs/languages.json").read_text(encoding="utf-8"))
    sources = {p.relative_to(root / ENGLISH).as_posix(): p
               for p in (root / ENGLISH).rglob("*.md")
               if p.name != "README.md" and "generated" not in p.parts
               and p.relative_to(root / ENGLISH).as_posix() != "reference/commands.md"}
    expanded = {name: english_page(path, root) for name, path in sources.items()}
    snapshots_path = root / "docs/.translation-sources.json"
    snapshots = json.loads(snapshots_path.read_text(encoding="utf-8")) if snapshots_path.exists() else {}
    needed = set()
    result = {}
    for language in languages[1:]:
        code = language["id"]
        folder = root / "docs" / code
        folder.mkdir(parents=True, exist_ok=True)
        if review == code:
            for target in folder.rglob("*.md"):
                if BEGIN in target.read_text(encoding="utf-8"):
                    raise ValueError("Integrate and remove English update blocks before marking " + code + " reviewed: " + str(target))
        manifest = folder / ".translation.json"
        state = json.loads(manifest.read_text(encoding="utf-8")) if manifest.exists() else {"pages": {}}
        state.setdefault("revision", revision)
        pages = state["pages"]
        pending = []
        commits = set()
        unknown = False
        for name, source in sorted(sources.items()):
            target = folder / name
            english, dependencies = expanded[name]
            current = hashlib.sha256(english.encode()).hexdigest()
            snapshots[current] = english
            previous = pages.get(name)
            untouched = target.exists() and (digest(target) == current or
                        (previous and digest(target) == previous.get("copy")))
            if not target.exists() or untouched:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(english, encoding="utf-8", newline="\n")
                untouched = True
            if previous is None:
                previous = {"source": current, "revision": state["revision"]}
                pages[name] = previous
            if untouched:
                previous["copy"] = current
            else:
                previous.pop("copy", None)
            if review == code and not untouched and target.read_text(encoding="utf-8").strip():
                previous.update(source=current, revision=revision)
                previous.pop("pending_copy", None)
            if not untouched and (previous["source"] != current or previous.get("pending_copy")):
                baseline = snapshots.get(previous["source"])
                if baseline is None:
                    try:
                        baseline, _ = english_page(source, root, reader=lambda p: git(root, "show", previous["revision"] + ":" + p.relative_to(root).as_posix()))
                    except subprocess.CalledProcessError:
                        # On migration without an available baseline, present the entire
                        # current source for review rather than guessing what was added.
                        baseline = ""
                if name == "SUMMARY.md":
                    target.write_text(update_summary(english, target.read_text(encoding="utf-8"), baseline), encoding="utf-8", newline="\n")
                else:
                    insert_updates(target, baseline, english, previous, code)
            needed.update((current, previous["source"]))
            reason = "untranslated" if untouched else "changed" if previous["source"] != current else None
            if not reason and BEGIN in target.read_text(encoding="utf-8"):
                reason = "English update block awaiting review"
            if reason:
                pending.append({"page": name, "reason": reason})
                try:
                    commits.update(git(root, "rev-list", previous["revision"] + "..HEAD", "--", *sorted(dependencies)).split())
                except subprocess.CalledProcessError:
                    unknown = True
        for name in sorted(set(pages) - set(sources)):
            if review == code:
                del pages[name]
            else:
                pending.append({"page": name, "reason": "removed from English; review manually"})
        manifest.write_text(json.dumps(state, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        result[code] = {"name": language["name"], "commits_behind": None if unknown else len(commits), "pending": pending}
    snapshots_path.write_text(json.dumps({key: snapshots[key] for key in sorted(needed) if key in snapshots}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    lines = ["# Documentation translation status", "", "Generated by `scripts/docs/sync.py`. English copies are ready to translate.", "",
             "| Language | English documentation commits behind | Pages to review |", "| --- | ---: | ---: |"]
    for code, status in result.items():
        count = status["commits_behind"]
        lines.append(f"| {status['name']} ({code}) | {count if count is not None else 'unknown (history unavailable)'} | {len(status['pending'])} |")
    for code, status in result.items():
        lines.extend(["", "## " + status["name"], ""])
        lines.extend(f"- [{p['page']}]({code}/{p['page']}): {p['reason']}" for p in status["pending"])
        if not status["pending"]:
            lines.append("Up to date.")
    text = "\n".join(lines) + "\n"
    (root / "docs/TRANSLATION_STATUS.md").write_text(text, encoding="utf-8", newline="\n")
    return result, text


def notify(result):
    """Maintain one bot-owned issue per language; never overwrite a human issue."""
    repository = os.environ["GITHUB_REPOSITORY"]
    def gh(*args):
        return subprocess.check_output(["gh", *args, "--repo", repository], text=True)
    issues = json.loads(gh("issue", "list", "--state", "all", "--author", "github-actions[bot]", "--limit", "1000", "--json", "number,body,state"))
    for code, status in result.items():
        marker = f"<!-- docs-translation:{code} -->"
        existing = next((i for i in issues if marker in (i["body"] or "")), None)
        if not status["pending"]:
            if existing and existing["state"] == "OPEN":
                gh("issue", "close", str(existing["number"]))
            continue
        behind = status["commits_behind"]
        body = marker + f"\nEnglish documentation commits behind: {behind if behind is not None else 'unknown'}.\n\n"
        body += "New pages and untouched English copies are synchronized through the documentation sync PR. New and changed English sections are appended to translated pages for review. Existing translations are preserved. Integrate and remove the marked English update blocks before marking pages reviewed.\n\n"
        body += "\n".join(f"- [ ] `docs/{code}/{p['page']}` — {p['reason']}" for p in status["pending"])
        body += f"\n\nAfter updating translations, run `python3 scripts/docs/sync.py --review {code}` and commit the manifest. See docs/i18n.md.\n"
        if existing:
            if existing["state"] != "OPEN":
                gh("issue", "reopen", str(existing["number"]))
            if existing["body"] != body:
                gh("issue", "edit", str(existing["number"]), "--body", body)
        else:
            gh("issue", "create", "--title", f"docs: {status['name']} translation updates", "--body", body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--review", help="Acknowledge the current English source for edited pages in this language")
    parser.add_argument("--revision", help="Source revision when initializing tracking")
    parser.add_argument("--notify", action="store_true", help="Maintain GitHub reminder issues (requires gh and GH_TOKEN)")
    args = parser.parse_args()
    root = args.root.resolve()
    ids = [l["id"] for l in json.loads((root / "docs/languages.json").read_text(encoding="utf-8"))][1:]
    if args.review and args.review not in ids:
        parser.error("--review must name a configured translation language")
    result, text = synchronize(root, args.review, args.revision)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(text)
    if args.notify:
        notify(result)
    print(text)


if __name__ == "__main__":
    main()
