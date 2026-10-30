# Command plugin test guide

This guide exercises the experimental native Wasm command host with the independent
`example-box` plugin. The plugin creates an ordinary 20 x 10 x 3 mm box. It does not
implement the reconstruction, conversion or mesh UI proposed in #8061.

## What you need

Use a Windows x64 test distribution built from this branch, extracted into a writable
local folder. The distribution contains the GUI, CLI, a compiled component, scripts,
source, dependency notices, this manual and file checksums. It is an unsigned test
build, not an official release or installer. Running its checks does not require Rust,
wasm-tools, an AI account or additional network permissions.

The source checkout alone does not contain the executables or compiled component.
Developers can build them using the instructions below. A package must contain a
WebAssembly **component**, not just a core Wasm module.

## Run the Windows test distribution

1. Extract the ZIP. In File Explorer, open the extracted folder that contains
   `Run-Plugin-Test.cmd` and `Start-SolidCraft-Test.cmd`.
2. Open `plugins/example-box/plugin.json` in a text editor. Its only command grant is
   `solid.box`; this example cannot read the existing design's mesh. Close the editor
   without saving changes.
3. Return to that folder in File Explorer. Press **Ctrl+L**, type `cmd`, and press
   **Enter**. A Command Prompt opens in the extracted folder. Type the following and
   press **Enter**:

   ```bat
   Run-Plugin-Test.cmd
   ```

4. Wait for the command to finish. Keep the Command Prompt open and look for this
   exact line:

   ```text
   PASS: 600 mm3 / one Undo / Redo / saved model / snapshot.
   ```

   `PASS` means the automatic checks succeeded. They inspect the exact bundled
   component, run it, check the volume, undo once, redo, save and reopen the design,
   and render an image. You do not need to inspect an exit code or operate Undo/Redo
   yourself. At the start, `Results:` prints the full output-folder path. Record that
   path: its `plugin-box.sc` is the file to open in the next step.

   If you see `FAILED. See ...` instead, stop here and include that line and the
   referenced `test.log` when reporting the failure. To read the log, copy its folder
   path from `Results:` into File Explorer's address bar, press **Enter**, right-click
   `test.log`, choose **Open with**, and select Notepad.

5. In the extracted folder, double-click `Start-SolidCraft-Test.cmd`. In the test GUI,
   press **Ctrl+O**, enter the full path to `plugin-box.sc` in the results folder you
   recorded, and open it. The box should appear as an ordinary editable native feature.

The command-line test creates a fresh results folder each time. The GUI launcher uses an
isolated application data and recovery profile under `results/gui-profile` and places the
sample in its isolated plugin folder.
Neither launcher replaces your normal SolidCraft settings or installed plugins.

The test GUI retains its settings under `results/gui-profile` between launches.
The English starter profile is copied only on first launch, so changes to plugin ceilings persist.
Use one test GUI at a time with this profile.
Automatic AI-app registration and update checking are off in this isolated profile.

| Check | Expected result |
|---|---|
| After `plugin.run` | One body, volume 600 mm3 |
| After one Undo | No bodies |
| After Redo | One body, volume 600 mm3 |
| Reopen the saved file | One body, volume 600 mm3; no plugin execution |
| Snapshot | An image of the box in the same results folder |

Undo/Redo in the script use one CLI session. Opening its saved design in a new GUI
session does not restore the CLI's session history. You can inspect and edit the
native box in the GUI. A saved result can be evaluated without the component installed.

## Inspect and run another package

`plugin.inspect` reads its static manifest and component bytes without executing
plugin code. Review the package, its command grants and limits before passing its
returned `approval` hash to `plugin.run`. The hash identifies exact content; it is
not a publisher signature or proof that a package is safe.

A CLI script is a JSON array. For example, save this as `inspect.json`, with a path
relative to the directory from which you run the CLI:

```json
[{"command":"plugin.inspect","params":{"path":"plugins/example-box"}}]
```

Run `bin\solidcraft-cli.exe run inspect.json --results --json-out inspection.json`.
Copy the `approval` value from the first result into a separate `run.json`:

```json
[{
  "command":"plugin.run",
  "params":{
    "path":"plugins/example-box",
    "approval":"COPY_THE_EXACT_APPROVAL_FROM_INSPECTION",
    "entry":"make",
    "params":{"length":20,"width":10,"height":3}
  }
}]
```

Then run `bin\solidcraft-cli.exe run run.json --results --save box.sc`.
Changing even whitespace in the manifest requires inspecting and reviewing it again.
A desktop control client must send `plugin.run` as a standalone `engine.execute`
request; desktop batches and scripts refuse it before applying any earlier edits.

## GUI behavior and current boundaries

Preferences > Plugins lists inert package metadata. Listing or enabling a package
never executes it. The CLI/MCP and standalone desktop command are the execution
entry points; this version has no dedicated plugin action panel or installer.
Desktop plugin calls compute on a worker. Stop, a changed design/selection/context,
or changed disabled-plugin preferences or resource ceilings prevent the pending result from
being adopted. The CLI is a separate process and does not inherit the GUI's plugin settings.

Guest code receives no WASI filesystem, network or process imports. Its exact
manifest grants are intersected with host policy. Commands run on a scratch design
under an empty file scope; a refused call, trap, invalid result or exceeded budget
discards the transaction, including errors caught by the guest. A successful edit
is committed as one undo step.

The optional `model.mesh` grant returns an evaluated body's component-local vertices
and triangle indices, up to 8192 vertices and 16384 triangles, subject also to a
1 MiB result cap. It contains no reconstruction or conversion algorithm. The bundled
box sample does not request that grant.

The experimental API is `solidcraft:plugin@0.1.0`. Defaults are 10 million fuel,
64 MiB guest memory and a 5000 ms execution deadline. Component compilation and native
CAD work are outside the guest execution budget. Browser execution, automatic
updates, custom panels, persistent guest storage and plugin-defined parametric
feature types are not implemented. See [the runtime reference](plugin-runtime.md)
for exact limits and supported commands.

## Explicitly allow a larger plugin budget

The box sample uses the unchanged invocation defaults; it needs no settings changes. Use the
following steps only for a separate package whose reviewed manifest requests a larger budget.
The original host ceilings are 100,000,000,000 fuel and 128 MiB. Raising either ceiling does
not change the sample's request or run any plugin.

For the desktop:

1. Inspect the package with `plugin.inspect` and read `manifest.runtime.limits`, `host_limits`,
   `within_host_limits` and any `limits_error`. This step does not execute code.
2. Open **Preferences > Plugins**. Enter a positive integer in **Max plugin fuel** and in
   **Max plugin memory (MiB)**, large enough for the specific request you reviewed. For
   example, a request of 200,000,000,000 fuel and 256 MiB fits ceilings of 200000000000 and 512.
3. Choose **Apply** to accept both fields and keep the dialog open, or **OK** to accept and
   close it. Enter also accepts both fields. Invalid or incomplete values keep the dialog open.
   **Cancel** restores changes since the last Apply; **Restore defaults** resets this page.
4. Inspect the package again to check `within_host_limits: true`. Review its exact manifest,
   code and grants, then invoke `plugin.run` with that inspection's `approval`. A host ceiling
   is not a substitute for approval of external code. The package still uses only its declared
   budget. If the manifest changes, inspect and approve the new content again.

For a standalone CLI:

1. Open Notepad, paste the JSON below, and save it as `plugin-limits.json` in your extracted
   test package folder. In the Save dialog choose **All files**, so the name does not gain
   `.txt`; select **UTF-8** encoding.

   ```json
   {"max_fuel": 200000000000, "max_memory_mib": 512}
   ```

2. Open a terminal in that folder and include `--plugin-limits plugin-limits.json` in each
   command that should use these ceilings, for example:

   ```text
   bin\solidcraft-cli.exe --plugin-limits plugin-limits.json run inspect-plugin.json --results
   ```

   Here `inspect-plugin.json` is the inspection script for the package you are reviewing,
   following **Inspect and run another package** above. Keep its package path and reviewed
   approval hash in the run script; use the same startup flag for that run command. Missing
   settings keys use the original ceilings; omitting the flag uses both original ceilings.
3. Headless MCP uses the same flag with `mcp --headless`. For a connected desktop, change the
   desktop preferences instead; the CLI refuses to override a connected application's policy.

Fuel accepts positive integers through 18446744073709551615. Memory accepts 1..4096 MiB because
this runtime uses Wasm32. These are guest limits; component compilation, native CAD work and
other SolidCraft memory are separate. Choose values suitable for available machine resources.
The execution deadline remains independently capped at 30 seconds.

## Build from source

Build the native GUI/CLI following [the build guide](building.md):

```sh
cargo build --locked --profile ci -p solidcraft -p solidcraft-cli
```

Build and componentize the independent guest following
[the example README](../examples/plugins/example-box/README.md). It documents the
Rust target and wasm-tools commands. Copy the resulting component and `plugin.json`
into a folder named `example-box`; its folder name must match the manifest id.
Use `plugin.inspect` to obtain the actual hash rather than copying a hash from a
different build. Native binary output paths depend on Cargo's target configuration.

## Troubleshooting

- **No result window:** use Command Prompt as described in step 3. Double-clicking
  the test CMD may close its window before you can read it.
- **Approval mismatch:** restore the bundled files, or inspect and review modified
  package contents before using their new hash.
- **Component compilation failed:** verify this is a compatible WebAssembly component,
  not a core Wasm module, DLL or executable.
- **Command is not granted:** the exact command must be permitted by both the manifest
  and the host. There are no wildcard grants.
- **Desktop asks for a standalone request:** send one `engine.execute` containing
  `plugin.run`, outside `engine.batch` and `engine.script`.
- **Check build identity:** consult the distribution's `BUILD-INFO.json`,
  `SHA256SUMS.txt` and the checksum beside its ZIP. Keep the generated `test.log`
  with any failure report. Test distributions are unsigned and are not releases.
