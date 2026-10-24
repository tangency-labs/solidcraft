# Report template

Fill in every section and delete the hints in angle brackets. Keep it short: the script and the numbers say most of it.

````markdown
**Reproduced on:** SolidCraft <version> (<release, or built from source at commit …>), <OS and version>, <GPU, for a display problem>
<or: **Only in my design:** <what was tried: its history as a script rebuilds fine / fails the same way>>

### Steps
1. <app step, in the app's words: Sketch → Rectangle on the XY plane, 20 × 20 mm>
2. <…>
3. <the step that fails>

Minimal script (`solidcraft-cli run repro.json`):

```json
{"commands":[
  <the smallest command list that still fails>
]}
```

### Expected
<what should happen, with the number you computed>

### Actual
<what happens, with the measured numbers>

```text
<the error text or command output, copied as it is>
```

<how often: every time / 3 of 5 runs; the boundary if you found one>

### Attachments
<screenshot of the failure; the crash report id if there is one; no private designs unless the user agreed>
````

## Example

The shape filled in, for a command that fails with an error. (This one is expected behaviour; a real report would be one where Actual differs from Expected.)

````markdown
**Reproduced on:** SolidCraft 0.18.2 (release), Linux x86_64

### Steps
1. Box, 20 × 20 × 10 mm, corner at the origin.
2. Fillet the top front edge, radius 12 mm.

Minimal script (`solidcraft-cli run repro.json`):

```json
{"commands":[
  {"command":"solid.box","params":{"length":20,"width":20,"height":10}},
  {"command":"solid.fillet","params":{"edges":[[10,0,10]],"radius":12}}
]}
```

### Expected
The fillet fails and names the largest radius that fits: 10 mm, the box's height.

### Actual
```text
error: step 2 (solid.fillet): not supported yet: the blend is larger than the neighbouring faces; the largest radius that fits is 10 mm
```
Every time. Radius 2 works: volume 3982.8 mm³, as computed by hand (4000 − 20 × 2² × (1 − π/4)).
````
