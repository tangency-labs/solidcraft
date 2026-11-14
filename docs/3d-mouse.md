# 3D mouse

On Windows, macOS and Linux, SolidCraft detects a connected 3D mouse through its HID multi-axis
controller interface: a 3Dconnexion SpaceNavigator or SpaceMouse, or a homemade device that
declares itself one. Move the cap to pan and zoom, and tilt or twist it to rotate the 3D view.
By default, zoom can pass through the model and orbit turns around the view's centre, as in earlier
versions. Two preferences opt into holding the model:

- Preferences › Navigation › **Stop zoom at the model** (`zoom_stop_at_model`, off by default)
  makes the cap and navigation bridge zoom toward the surface in the middle of the view and stop
  there. Drag-zoom holds the model point where the drag began. The wheel and pinch keep their
  separate **Zoom to the cursor** preference; neither setting changes the other.
- Preferences › Navigation › 3D mouse › **Orbit** defaults to **Classic**. Choose **Traced**
  (`space_mouse_model_pivot: true`) to turn around the model point in the middle of the view when
  the turn begins. A faint ball marks that point until the cap is back at rest. Over empty space,
  the pivot is the middle of the bodies in view. The navigation bridge uses the same preference,
  but picks the point each frame because device apps do not report rest.

While editing a sketch, the cap pans and zooms without changing the sketch's initial view. After you
adjust the view with the navigation cube, the cap can rotate it too. Turn on **Turn sketches with
the 3D mouse** (`space_mouse_sketch_orbit: true`) to let the cap rotate a sketch seen square on as
well, as the mouse can. Turn on **Keep the horizon level with the 3D mouse**
(`space_mouse_horizon_lock: true`) and the view never rolls: Tilt turns it up and down, Roll and
Spin turn it about the world's up axis, and a view that is already rolled is eased level. With
**Free orbit** on as well, the tilt goes on over the poles and the horizon stays level. The Navigation
page in Preferences has an on/off switch, a speed control and, for each of the cap's six axes, an
invert switch and a speed for the 3D mouse. The cap's zoom direction comes from its own invert
switch; **Reverse zoom direction** under Wheel and trackpad reverses the wheel only (#8993). The web app reads
it too, through the browser; Firefox needs a 3DxWare setting first (see
[Getting started › 3D mouse in the browser](book/src/guide/getting-started.md#3d-mouse-in-the-browser)).

With 3DxWare on Windows, SolidCraft follows 3DxWare's profile for it when there is one, and
3DxWare's object mode (tilt to roll, twist to spin) until there is. The mouse-wheel turns that
3DxWare makes from the cap's motion are ignored, so the view doesn't zoom in steps as the cap tilts.

On Linux SolidCraft reads the 3D mouse through `spacenavd` when that daemon runs (its socket
`/var/run/spnav.sock`), so what you set in `spnavcfg` (swapped or inverted axes, sensitivity, dead
zones) applies as it does in other programs, and no udev rule is needed. With nothing set there,
the motion is the same as without the daemon. Without `spacenavd`, SolidCraft reads `/dev/hidrawN`
directly, so the logged-in user needs access to the device. A udev rule that adds the `uaccess`
tag does that, for example
`/etc/udev/rules.d/70-3d-mouse.rules` for USB devices with 3Dconnexion's vendor id, and for its
Bluetooth ones (where `idVendor` isn't set, so the HID id is matched instead):

```
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="256f", TAG+="uaccess"
SUBSYSTEM=="hidraw", KERNELS=="0005:256F:*", TAG+="uaccess"
```

Older SpaceNavigators use Logitech's vendor id `046d`, so add a rule with their product id (see
`lsusb`) instead of matching every Logitech device. A homemade device needs a rule with its own
vendor and product id. Reload with `sudo udevadm control --reload && sudo udevadm trigger`, then
replug the device. The Flatpak sandbox does not expose hidraw devices, so the 3D mouse only works
with native builds there for now.

On Windows and macOS, with more than one 3D mouse connected, SolidCraft reads the one in use.
For example, a 3Dconnexion Universal Receiver with nothing paired may sit next to a SpaceMouse
on a cable. The first device found is read until another one moves, and then that one is. The
Navigation page names the device being read, and its calibration applies.

SolidCraft finds the axes from the device's report descriptor, so it doesn't matter how a device
lays them out: translation and rotation in separate reports (3Dconnexion's wired devices), all six
in one (its wireless ones and most homemade devices), after the buttons, or narrower than 16 bits.
Each axis is scaled from the range the descriptor declares.

On macOS SolidCraft talks to the 3Dconnexion driver (3DxWare) when it is installed, through the
driver's client API (`crates/threedx-client`; the framework is loaded at run time, never bundled).
The driver keeps the device to itself, and sends it only to a program it can identify: SolidCraft
as an `.app` (the packaged app). Without the driver, or with its helper not running, SolidCraft reads the device through IOKit like on Windows.

SolidCraft does not follow 3DxWare's profile on macOS. Its default is 3DxWare's object mode.

## Device apps: the navigation bridge

Some homemade input devices come with a program of their own that reads the device and turns its
knobs, rings and buttons into navigation (profiles, sensitivity, button actions). Such a program can
move SolidCraft's view directly instead of emulating mouse drags, so the mouse pointer stays the
user's and both can be used at once. Turn on **Navigate with a device app on this computer** in
Preferences › Navigation › 3D mouse (off by default) and set the port the program listens on
(8765 unless the program says otherwise). The desktop app then connects to `127.0.0.1:<port>`,
retries every second while nothing answers, and shows **Connected** with the program's name once it
is. Only the window with focus moves, and not on the start page or while a file dialog is open.

The program is the server; SolidCraft connects and first sends one line,
`{"t":"hello","app":"solidcraft","version":"<version>"}`. Then the program sends one JSON object per
line (UTF-8, `\n`-terminated, at most 4096 bytes); anything else, and any message SolidCraft doesn't
know, is ignored:

| Message | Meaning |
|---|---|
| `{"t":"hello","server":"MAKMOUSE","device":"MAKMOUSE"}` | The program's name (shown in Preferences) and, optionally, a name in the device's USB manufacturer or product string. A 3D mouse with that name is then left to the program, so its motion doesn't reach the view twice. |
| `{"t":"axis","x":4.0,"y":-2.5}` | Motion since the last message, in points: it orbits as a drag of that many points would (the model follows), or pans or zooms in the current mode. Where the view can't turn (a sketch being edited and seen square on, unless **Turn sketches with the 3D mouse** is on; a flat page) an orbit pans. |
| `{"t":"zoom","d":1.0}` | Zoom steps (8 % of the distance each); positive zooms in, as a wheel turned forward. Reverse zoom direction applies. Stops at the model only when **Stop zoom at the model** is on. |
| `{"t":"mode","mode":"pan","on":true}` | What `axis` drives: `orbit` (the default), `pan` or `zoom` (up zooms in, 8 points a step). `"on":false` goes back to orbit. |
| `{"t":"view","name":"iso"}` | A standard view: `front`, `back`, `left`, `right`, `top`, `bottom`, `iso`, or `fit` to frame everything. |

Values that aren't finite numbers count as none, and each is limited to ±10 000 per message and per
frame. The protocol is the one the MAKMOUSE driver app uses for its add-ins.
