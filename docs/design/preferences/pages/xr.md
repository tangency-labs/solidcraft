# XR

No board: the page was added with the VR viewer (#2172, `docs/vr.md`) as a proposal, open to
feedback. Named XR, not VR: VR is its only setting for now, and AR or other XR features would go
on the same page. Heading: Input, after the 3D mouse: like a 3D mouse, few people have a headset, and VR
will need many settings, so they get a page of their own rather than rows on another. With it
the page list no longer fits the window's height and scrolls. Subtitle "Show the design in a VR headset and
work on it with the controllers." No "In profile" tag (the headset stays on this computer). Source:
`crates/ui-egui/src/prefs/vr.rs`.

Where the host has no VR (macOS, the browser), the page holds only the note "VR needs Windows or
Linux, and VR software with OpenXR (SteamVR, Meta Quest Link)."

## Start

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Start in VR + badge "Restart needed" | Checkbox | `Prefs.vr_headset` (saved in `graphics.json` as `vr`) | `false` | "From the next start, the design stands in the room in front of you in the VR headset, and you move it with the controllers; ..." | none | Read at start, before the window opens. |
| (status) | Hint text | none (`Services::vr_status`) | n/a | none | none | What this run is doing: waiting for the headset, showing in it, or why VR did not start; without VR, "OpenXR found" or "OpenXR not found: " and what is missing. |
