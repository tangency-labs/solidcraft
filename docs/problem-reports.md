# Problem reports

Report a bug (the title bar's button), Help › Report an Issue, a dialog's Create Issue and the
Crash Reports window open **Report a problem**: an in-app form for a ticket on
[tickets.solidcraft.io](https://tickets.solidcraft.io). Only the reporter, triagers and admins
can see a ticket unless the reporter makes it public.

The form has the app (SolidCraft unless another is picked), the type (bug, feature or question),
a title and a description. From a failed operation it arrives filled in: the title is
"<Command>: <error>", the description names the failed command, its key settings and the error,
and ends with "What were you trying to do?" (dropped on Send when nothing was written after it).
A long error is cut at a line break or a space and ends with "…"; the whole error goes with the
failed operation details.

Right under the description is **Include your model (.sc file)**, off by default: the design with
its full history, which is what reproduces a problem. A report made from a crash report attaches
the crashed session's design as the crash report kept it (its last autosave or its file), never
the design open now; when the crash left no copy, no design is offered and the dialog and the
details say so. When the open design is new and empty and a design from a run that ended (a
crash, a lost graphics device) waits for recovery, that design is offered instead. Left off on a report about a failure, the
dialog says plainly that the failure probably can't be reproduced without it. Below it, each other
attachment is a checkbox with what it holds, shown before it goes:

| Attachment | Default | What it holds |
| --- | --- | --- |
| Screenshot of the 3D view | on | The view as it was when the report opened (Retake updates it). |
| Log | on | The last 500 lines (at most 128 KB) of the app's log file, `<data folder>/logs/solidcraft.log`, after the end of the run before's (`solidcraft.previous.log`) when this run has fewer; the home folder shown as `~`. A report made from a crash report sends the end of the crashed run's own log instead, from the last 5 runs' logs the app keeps. The log has SolidCraft's info lines and other libraries' warnings: every command and how it went, and every message shown. The web build, which has no log file, sends the command log and the Messages. |
| System info | on | App version, commit, target, renderer, graphics card and driver, window size, language. Nothing about the design. A report made from a crash report names the crashed run's graphics card, driver and graphics API, and this run's under `reported_from`. |
| Failed operation details | on | `session_start` (new, opened, recovered or crashed), the attached design's SHA-256 (not the file) or why there is none, its format version, undo and redo steps, the recent `steps` (the last 50 commands of the session, or the crash report's, with whether each worked; their parameters, which name things in the design, go only when the model is included, and a command about files keeps only the file's name) and the failed command with its exact parameters and error. |
| Crash report | on | Offered for a report made from a crash report: its `report.json` (the internal error, where it happened, the startup step, the native module and offset, the renderer, the commands before it). Untick it to leave it out. The minidump is never sent; it stays in the crash folder. |

The ticket carries a `consent` record listing exactly which boxes were ticked.

## Sending

**Send** creates a report draft on the server (no account needed), uploads the ticked
attachments with the draft's upload token (resumable, SHA-256 checked) and opens the draft's page
in the browser. The person signs in there (with GitHub or Google; the account is made on first
use), reviews the report and clicks File ticket. The dialog shows each stage in place of the form:
Sending… with a progress bar and Cancel, then Thanks with the draft's link (Open it again, Copy
link), or the exact reason it failed with Retry and Copy details. Each step is written to the
log. Send is never refused for an empty title: the description's first line, or "Problem report",
stands in.

SolidCraft keeps no account details: no sign-in and no token. A keyring entry "SolidCraft
tickets" left by a test build is deleted once at startup.

A report that cannot reach the server (offline, a timeout, a busy or failing server) is kept in
`<data folder>/report-queue` (at most 20 reports and 1 GB) and sent when the person presses
Retry. A report the server refuses leaves no files behind.

## From an agent

An agent driving SolidCraft (MCP or the control channel) files a report with
`diagnostics.report_issue {title, description?, type?, app?, system?, repro?, log?, screenshot?,
design?, crash?}`; it should ask the person first. The app gathers what the dialog would (the
screenshot is a render of the view), sends it as a draft, opens it in the person's browser and
answers `{report, status: "pending"}`. `diagnostics.report_issue_status {report}` says where it is:
`sending`, `opened` (with the draft's `url`), `filed`, `queued` (offline) or `failed`.

An agent cannot agree to send the person's design or crash report. Asking for them (`design` or
`crash: true`) sends nothing: the report dialog opens filled in with those boxes unticked
(`awaiting_user`), and only the person can tick them and press Send report. The ticket's
`consent` records `via: "agent"` and, when the person sent it from the dialog,
`confirmed_by: "user"`. The command needs the app; a headless session refuses it.

## Replaying a failure

The attached `.sc` holds the design's whole history, so a failure replays on the same build
(`system.commit`) with the failed command from `repro.failing`:

```sh
echo '{"commands":[{"command":"solid.fillet","params":{...}}]}' > replay.json
solidcraft-cli run replay.json --in design.sc
```

## Network use

The bug reporter talks to the ticket server only when the person sends a report (or Retry), or an
agent asks for one; it sends nothing in the background. Anonymous usage statistics are separate:
they go to their own service, never the ticket server ([Privacy](privacy.md)).

## The old behaviour

The dialog's "Or open a public GitHub issue instead" link switches it to the public GitHub issue
draft the button opened before, with the same title and description.
Preferences › Security › "Report a bug opens a public GitHub draft instead"
(`report_on_github`) makes every report the GitHub draft, as before.

## Server API

Every path the app uses is in `crates/ui-egui/src/tickets/api.rs`: report drafts (`POST
/api/drafts`, with `system`, `repro` and `consent`, no account) and resumable uploads with the
draft's upload token (`POST /api/uploads` with the file's size and SHA-256, `PUT
/api/uploads/:id/parts/:n` in the parts the server asks for, `GET /api/uploads/:id` to resume,
`POST /api/uploads/:id/complete`). The draft and its page must be on the ticket server itself
(host compared exactly). `SOLIDCRAFT_TICKETS_URL` points a desktop build at a test server
(`https://`, or `http://` to 127.0.0.1 or localhost).
