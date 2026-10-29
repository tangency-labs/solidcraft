# Contributing command guides and resources

The command reference combines the live command registry with reviewed Markdown
guides. A guide appears **inside its command's entry**, before the collapsed
automation syntax. You can improve one entry without rewriting the whole manual.
The reference's **Practical guides** list automatically links to every available
guide; adding a guide does not require editing an index or Rust code.

## Add or improve an entry

1. Find the command in the [reference](../reference/commands.md). Its ID is in
   **Automation details**.
2. Copy the [guide template](https://github.com/tangency-labs/solidcraft/blob/main/docs/book/command-guide-template.md)
   to `docs/book/command-guides/<command-id>.md`, or edit its existing file.
   For example, Extrude lives in `solid.extrude.md`.
3. Explain what the tool does, what must exist or be selected first, the dialog's
   choices in a table, a short example, and useful DOs and DON'Ts. Use plain
   language rather than copying the raw automation parameters.
4. Add related commands and any checked community resources, then open a pull
   request. Documentation changes are reviewed like code.

Do not edit the generated `src/reference/commands.md`: rebuilding replaces it.
The generator rejects guide filenames whose command is absent from the registry.
Names, groups, shortcuts and automation parameters continue to come from that
registry; handwritten explanations remain editable independently.

Use fourth-level headings (`####`) inside an entry so the command remains the
parent section. Write image and page paths relative to the **generated reference
page**, `docs/book/src/reference/commands.md`, not to the input guide file.
For example, `../generated/sheet-metal.png` and `../guide/sheet-metal.md`.

## Related commands

Write a command link as `{{#command sheet.hem}}`. The generator inserts its
current name and links directly to that command. Unknown or missing command IDs
and unfinished directives fail the book build, helping contributors catch stale links.
Normal Markdown links can point to guide pages or external resources.

## Community resources

You can add a focused tip, a short demo, a model or an illustrative screenshot
to the entry's resource table. Explain what the reader learns from each link;
include author and licence information. Links are reviewed, not automatically
collected from the web. A resource need not cover the whole command.

Use original SolidCraft examples or openly licensed resources with permission
to share. Do not use designs, screenshots, help text or artwork extracted from
commercial CAD products, or GPL/LGPL/AGPL code. Prefer small public-safe examples
and remove private designs, paths and account information from evidence.

For a new file asset, follow `assets/AGENTS.md` and add its attribution in a new
`attribution/pending/<change>.md` file. A reproducible recipe in
`docs/book/scripts/` can be rendered by the book builder; generated images stay
out of Git. Existing examples demonstrate both inline pictures and resource links.

## Preview and review

Run `cargo xtask book` with mdBook installed, inspect the command entry and its
links, and check the example in SolidCraft. Confirm the required selection,
units, defaults and limitations against the implementation. Show reviewers an
actual before/after screenshot of the rendered entry. Repository quality gates
still apply before a commit.

## Copyable template

````markdown
{{#include ../../command-guide-template.md}}
````
