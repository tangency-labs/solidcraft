# Tutorial mode

Open **Tutorial** on the start page, under Crash Reports, to see what SolidCraft's functions do.

Every function the ribbon shows has a lesson: 295 of them. The tutorial window groups them as
the ribbon does (Model › Create, Sketch › Constraints, Surface › Modify…). Type in **Find** to
search the lessons by name or by what the tool does.

## Seeing a function

Click a function in the window, or its ribbon button while the tutorial is open. SolidCraft
builds the sample model that function needs in a **Tutorial** tab, then acts the function out:

1. A **Tutorial** pointer clicks the command's ribbon button, with its keyboard shortcut shown
   as pressed keys (for example Alt + B for Chamfer).
2. The pointer clicks the edges, faces or points the function works on.
3. The value it types appears as keys (for example 4 + Enter).
4. The function runs.

The view frames the finished result before the lesson starts, so you see everything the tool
makes. Some lessons have no demo to act out — a view toggle, a list, an editor whose panel is
the whole point — and those set the sample up and open the tool itself. The samples include:

- plates, pins, rods and blocks
- sketches, profiles and 3D sketches
- surfaces
- a sheet metal part, flanged and unfolded
- a shelled case and a plastic rule set
- assemblies with joints, gears and contact sets
- a frame sketch, and a scanned mesh
- sample STEP, STL and design files for the insert tools

The window then explains what happened and shows the function's keyboard shortcut. Measure,
Design Properties and Compare Bodies also show their numbers.

**Voice narration** reads each tool's explanation aloud, says what the demo is doing as the
pointer works, and reads the result. It is on by default and the tick box in the window turns it
off.

There are two voices. SolidCraft bundles a neural one (Kokoro, under `assets/tts/`), which sounds
like a person reading and runs entirely on your computer — nothing is sent anywhere. It is
synthesized a line at a time on a background thread, and the demo waits for it: the pointer does
not act until the line naming the tool has begun, and the next step does not start until that line
has finished, so what you hear and what you see stay together. Where that voice cannot load — no
model beside the program, no sound device — the operating system's own speech voice reads instead,
and the demo runs at its own pace. A computer with neither simply stays silent.

The lines are English, whatever the interface language — they are built
by gluing fragments together, and a sentence assembled that way cannot be made to work in another
language; a translated narration needs whole sentences written per language, with a voice to read
them.

The window has three buttons:

| Button | What it does |
|---|---|
| **Replay** | Builds the sample again and acts the function out again. |
| **Try it yourself** | Sets up the same sample and starts the command, so you pick the edges, faces or points and set the values. |
| **Next** | Moves to the next function. |

Undo (Ctrl+Z) steps back through the function that was just performed.

Each lesson replaces the Tutorial tab's design, so your own designs stay untouched in their own
tabs. A lesson's design is never counted as unsaved, so it isn't autosaved or offered for
recovery. Close the window with its × button or its Close action.

The tutorial's pointer is not the agent pointer (Preferences › AI). They look alike and work out
where to click the same way, but the agent pointer shows what an agent is doing to your design
and is held to a time budget per request, while this one is teaching: it goes at a pace you can
follow, and shows the keys a person would press.

## For scripts and AI agents

The control channel's `ui.tutorial` request runs the same lessons.
