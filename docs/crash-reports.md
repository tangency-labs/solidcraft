# Crash reports

When the desktop app crashes or hits an internal error, it writes a crash report on this
computer. Each report holds enough to reproduce and fix the problem: in most cases the design
and the exact commands that trigger it. Reports are never sent anywhere. To share one, use
**Report…** in Help ▸ Crash Reports…, which opens a GitHub issue draft. Nothing is shared
until you review and submit it, and the design is not part of the draft.

To send a report:

1. Review the title, report text and screenshot in SolidCraft, then click **Open GitHub draft**.
   For long reports, SolidCraft copies the text to your clipboard; paste it into the GitHub draft.
2. Attach the screenshot if you want to share it: use **Copy screenshot** and paste it into
   GitHub, or use **Save screenshot…** and upload the PNG. Screenshots are not attached automatically.
3. Click **Submit new issue** on GitHub to publish the report. Opening the draft alone does not send it.

## Where they are

`<data dir>/solidcraft/crashes/`, next to the recovery folder:

| System | Folder |
|---|---|
| Linux | `~/.local/share/solidcraft/crashes` (or `$XDG_DATA_HOME/solidcraft/crashes`) |
| Windows | `%LOCALAPPDATA%\SolidCraft\crashes` |
| macOS | `~/Library/Application Support/SolidCraft/crashes` |

`SOLIDCRAFT_CRASH_DIR` overrides the folder. `solidcraft --crash-log` and
`solidcraft-cli crashes dir` print it.

## For agents

All of these read the same folder:

| From | List | One report | Delete |
|---|---|---|---|
| Engine command (MCP `execute`, control channel, scripts) | `diagnostics.crash_reports {limit?}` | `diagnostics.crash_report {id}` | `diagnostics.crash_reports.clear {id?}` |
| MCP tool | `crash_reports {limit?}` | `crash_reports {id}` | — |
| CLI | `solidcraft-cli crashes [--json]` | `solidcraft-cli crashes show <id>` | `solidcraft-cli crashes clear [<id>]` |
| Control channel (UI) | `ui.help {"item": "crashes"}` opens the window | | |

**Reproduce:** `solidcraft-cli crashes replay <id>` opens the report's design, runs its
commands, and reports whether the same internal error happens again (same place and message).
Without the CLI, run `solidcraft-cli run <report>/replay.json --in <report>/design.solidcraft`.
A fixed crash turns into a regression test with the same two files.

**Check that reporting works:** with `SOLIDCRAFT_CRASH_TEST=1` set for the app, the command
`diagnostics.crash_test` has three modes:
- `panic`: an internal error in a command
- `kernel`: an internal error inside the geometry kernel
- `abort`: the app ends at once, and the report appears at the next start

## What a report holds

A folder `crashes/<YYYYMMDD-HHMMSS>-<signature>/`:

- `report.json` (`"format": "solidcraft-crash/1"`):
  - `kind`: one of
    - `command_panic`: a command hit an internal error; it was caught and the design was left unchanged
    - `kernel_panic_recovered`: the third-party geometry kernel panicked and was caught. This is harmless on the desktop, but it would crash the web build.
    - `panic`: an internal error outside any command
    - `unclean_exit`: a previous session ended without recording a clean exit. It is detected at the next start, which may be days later. Possible causes include an abort, a stack overflow, a segfault, a crash inside a graphics driver, a system shutdown, or the process being forcibly stopped. This kind alone does not identify the cause. When the app died while starting up, the message and `location` name the startup step (see below). On Windows, a captured native exception also says the exception, the module it happened in and the offset into it.
  - `message`, `location` (file:line), `thread`
  - `backtrace`: frames `symbol (file:line)`, without the panic machinery, std internals or the user's home folder
  - `app` (version, commit, build date), `system` (OS, architecture, renderer and GPU), `session.uptime_s`
  - `command`: the command that crashed, with its parameters
  - `recent_commands`: up to 32, with ok or error
  - `base`: what the replay starts from (the design before the command, an autosave, a saved file or a new design)
  - `count`: how many matching reports have been recorded; repeats update the existing report.
  - `first_seen`, `last_seen`: UTC timestamps when the report was first and most recently recorded. For `unclean_exit`, these are detection times at a later launch, not the exact times the sessions ended. The session start in `message` may be much earlier; the report does not imply a failure in the currently open design.
  - `notes`: what is missing and why, for example a design too large to copy
  - `startup_phase`: `{step, ms}` when the app died before it drew its first frame: the step it
    was in (`window`, `graphics instance`, `window surface`, `adapter request`, `adapter chosen`,
    `device creation`, `egui renderer (shader compile)`, `surface configure`, `app setup`,
    `viewport pipelines (shader compile)`, `first frame`, `first frame (present)`) and how many
    milliseconds after launch it began. `system.environment` names the renderer (backend, GPU,
    driver) as soon as the adapter is picked, so it is there even when the device never started.
  - `native` (Windows): what the exception filter caught: `code`, `name`, `address`, `module`,
    `offset` (into the module), `detail` (for an access violation, the address read or written)
    and `thread`. The `backtrace` then shows the same in two lines.
- `design.solidcraft`: the design the replay starts from
- `replay.json`: `{"commands": [{command, params}, …]}`
- `minidump.dmp` (Windows, native crashes): the threads' stacks and the loaded modules at the
  crash, for WinDbg or Visual Studio. It holds stack memory only, not the design, but it stays
  on this computer like the rest of the report; share it only on request.

A crash that ends the process without any exception (a fail-fast, which is how an abort ends on
Windows) still gets the startup step. Windows can also keep its own dumps of every crash: set
`DumpFolder` under `HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps`.

## How it stays cheap and small

- **Before a crash:** the app keeps a journal of the top-level commands run on the design on
  screen (`crashes/sessions/*.jsonl`, at most 512 KB). Each command adds two small writes,
  with no flush to disk. Previews and other scratch sessions are not recorded. Each autosave
  restarts the journal from the autosaved design. A clean exit deletes it. The journal file
  is locked while its app runs, so at the next start, a journal nobody holds belongs to an
  app that died.
- **A panic** captures its backtrace only the first time its signature is seen. The
  signature is the place plus the message, with numbers ignored.
- **Limits:**
  - at most 5 new reports per run
  - 20 reports
  - 50 MB in total
  - 90 days
  - a design over 10 MB is not copied; the report notes that

  When over a limit, recovered kernel panics are deleted before real crashes, and the oldest first.
- **Where it's off:** the CLI, tests and the web build never install the hook. There,
  commands pay only a check of one atomic flag.

Release builds keep line tables (`debug = "line-tables-only"`), so backtraces name the file
and line. Stripping a binary (`strip`) removes them, and the function names too.
On macOS the linker leaves the line tables in the build's object files, so the downloaded app
never had them: its backtraces name functions only. `packaging/macos/package.sh` removes just
the debug map that points at those files (`strip -S`); the function names stay.
