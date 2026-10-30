# Crash reports

When the desktop app crashes or hits an internal error, it writes a crash report on this
computer. Each report holds enough to reproduce and fix the problem: in most cases the design
and the exact commands that trigger it. Reports are never sent anywhere. To share one, use
**Report…** in Help ▸ Crash Reports…, which opens a GitHub issue draft. Nothing is shared
until you review and submit it, and the design is not part of the draft.

To send a report:

1. Review the title, report text and screenshot in SolidCraft, then click **Open GitHub issue form**
   (**Open GitHub** in narrow windows). SolidCraft copies the report text to your clipboard and opens
   GitHub. Short reports are also filled in through the URL; paste longer reports into the GitHub draft.
2. Attach the screenshot if you want to share it: use **Copy screenshot** and paste it into
   GitHub, or use **Save screenshot…** and upload the PNG. Screenshots are not attached automatically.
3. Click **Submit new issue** on GitHub to publish the report. Opening the draft alone does not send it.

### Submission and fallback behavior

The same reviewed report dialog is used by **Report a bug**, command errors and crash reports.
Desktop and web builds submit through GitHub in the browser; the dialog has no direct submission
API and needs no GitHub token in SolidCraft. It explains this beside the public-report/account notice.
Sign in with a GitHub account to publish the report.

**Copy report** and **Open GitHub issue form** remain available without credentials or a connection.
Copying works even before the required description is filled in; opening GitHub with an empty
description instead focuses that field and explains what is missing. Neither action closes the draft.
On narrow windows the footer uses **Open GitHub** and **Copy report**; close the draft with the header's
close button. The two export actions must remain visible at high UI zoom.

If offline, a browser launch is blocked, or GitHub cannot be reached, the dialog tells the user to
copy the report and keep the text to submit later. Opening GitHub requests a clipboard copy even for
short reports. SolidCraft cannot confirm browser launch, clipboard delivery or submission; the draft
stays editable for retrying. Copying or opening a report does not modify the design or its undo history.

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
Without the CLI, run `solidcraft-cli run <report>/replay.json --in <report>/design.sc`.
A fixed crash turns into a regression test with the same two files.

**Check that reporting works:** with `SOLIDCRAFT_CRASH_TEST=1` set for the app, the command
`diagnostics.crash_test` has three modes:
- `panic`: an internal error in a command
- `kernel`: an internal error inside the geometry kernel
- `abort`: the app ends at once, and the report appears at the next start. On macOS the
  signal handler catches it, so the report names SIGABRT and the frames that called `abort()`.

## What a report holds

A folder `crashes/<YYYYMMDD-HHMMSS>-<signature>/`:

- `report.json` (`"format": "solidcraft-crash/1"`):
  - `kind`: one of
    - `command_panic`: a command hit an internal error; it was caught and the design was left unchanged
    - `kernel_panic_recovered`: the third-party geometry kernel panicked and was caught. This is harmless on the desktop, but it would crash the web build.
    - `panic`: an internal error outside any command
    - `unclean_exit`: a previous session ended without recording a clean exit. It is detected at the next start, which may be days later. Possible causes include an abort, a stack overflow, a segfault, a crash inside a graphics driver, a system shutdown, or the process being forcibly stopped. This kind alone does not identify the cause. When the app died while starting up, the message and `location` name the startup step (see below). On Windows and macOS, a captured native crash also says the exception or signal, the module it happened in and the offset into it.
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
  - `native` (Windows and macOS): what the native crash handler caught: `code`, `name`,
    `address` (of the faulting instruction), `module`, `offset` (into the module), `detail` (for
    an access violation or a segfault, the address read or written) and `thread`. The `backtrace`
    then shows the same in two lines.
    - On macOS, where a signal handler catches SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGTRAP and SIGABRT,
      `code` and `signal` are the signal's name (`SIGSEGV`), `name` says it in words (`segmentation
      fault`, or `stack overflow` when the fault is just below the thread's stack), and it adds
      `si_code`, `thread_name`, `pid` and `frames`: up to 24 `module+offset symbol+offset` lines
      walked up the frame pointers from the faulting instruction, which the `backtrace` lists too.
      The process still ends with the signal, so macOS also writes its own crash report, with every
      thread's stack: `~/Library/Logs/DiagnosticReports/solidcraft-<date>.ips` (Console ▸ Crash
      Reports), whose `pid` is the one in `native`.
- `design.sc`: the design the replay starts from
- `replay.json`: `{"commands": [{command, params}, …]}`
- `minidump.dmp` (Windows, native crashes; on macOS, see the system's own report above): the threads' stacks and the loaded modules at the
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
