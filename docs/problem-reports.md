# Problem reports

Report a bug (the title bar's button), Help › Report an Issue, a dialog's Create Issue and the
Crash Reports window open **Report a problem**: an in-app form for a ticket on
[tickets.solidcraft.io](https://tickets.solidcraft.io). Only the reporter, triagers and admins
can see a ticket unless the reporter makes it public.

The form has the app (SolidCraft unless another is picked), the type (bug, feature or question),
a title and a description. From a failed operation it arrives filled in: the title is
"<Command>: <error>", the description names the failed command, its key settings and the error,
and ends with "What were you trying to do?".

Right under the description is **Include your model (.sc file)**, off by default: the design with
its full history, which is what reproduces a problem. Left off on a report about a failure, the
dialog says plainly that the failure probably can't be reproduced without it. Below it, each other
attachment is a checkbox with what it holds, shown before it goes:

| Attachment | Default | What it holds |
| --- | --- | --- |
| Screenshot of the 3D view | on | The view as it was when the report opened (Retake updates it). |
| Log | on | The last 300 lines of the design's command log and the Messages, with the home folder shown as `~`. |
| System info | on | App version, commit, target, renderer, graphics card and driver, window size, language. Nothing about the design. |
| Failed operation details | on | `session_start` (new or opened), the design's SHA-256 (not the file), its format version, undo and redo steps, and the failed command with its exact parameters and error. |
| Crash report | off | Offered for a report made from a crash report. |

The ticket carries a `consent` record listing exactly which boxes were ticked.

## Sending

**Send report** creates a report draft on the server (no account needed), uploads the ticked
attachments with the draft's upload token (resumable, with a progress bar and Cancel) and opens
the draft's page in the browser. The person signs in there if needed, reviews the report and
clicks File ticket; SolidCraft has nothing more to do. If they tick "Remember this computer"
there, the app keeps the token it is given in the system keyring (service "SolidCraft tickets";
the browser's local storage on the web) and later reports are filed directly, with "Ticket #N
filed" and an Open button. Preferences › Security › Problem reports shows which and signs out.

Send report is disabled from the click until the dialog closes. The same report (title, failed
operation and design) sent again within 30 minutes is not sent twice: the dialog says "You already
sent this report — open it again?" and opens the first draft. The server also answers a repeat with
the same draft (`duplicate`), and one already filed is not uploaded again.

A report that cannot reach the server is kept in `<data folder>/report-queue` and the dialog
offers Send now. With a remembered token it is sent by itself when SolidCraft starts or every
five minutes. The queue never holds a token.

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

SolidCraft talks to the ticket server only when the user sends a report or opens My tickets, to
send a report already waiting in the queue with a remembered token, or, while a draft it opened is
waiting to be filed, to check whether it was. There is no telemetry.

## The old behaviour

The dialog's "Or open a public GitHub issue instead" link switches it to the public GitHub issue
draft the button opened before, with the same title and description.
Preferences › Security › "Report a bug opens a public GitHub draft instead"
(`report_on_github`) makes every report the GitHub draft, as before.

## Server API

Every path the app uses is in `crates/ui-egui/src/tickets/api.rs`: report drafts
(`POST /api/drafts`, `GET /api/drafts/:id/status` with the upload token), device sign-in
(`/api/device/code`, `/api/device/token`, a `tickets:report` token), `/api/apps`,
`POST /api/tickets` (with `system`, `repro` and `consent`), and resumable uploads (`POST
/api/uploads` with the file's size and SHA-256, `PUT /api/uploads/:id/parts/:n` in the parts the
server asks for, `GET /api/uploads/:id` to resume, `POST /api/uploads/:id/complete`).
`SOLIDCRAFT_TICKETS_URL` points a desktop build at a test server (`https://`, or
`http://localhost`).
