# 3D mouse

On Windows, macOS and Linux, SolidCraft detects a connected 3D mouse through its HID multi-axis
controller interface: a 3Dconnexion SpaceNavigator or SpaceMouse, or a homemade device that
declares itself one. Move the cap to pan and zoom, and tilt or twist it to rotate the 3D view.
While editing a sketch, the cap pans and zooms without changing the sketch's initial view. After you
adjust the view with the navigation cube, the cap can rotate it too. The Navigation
page in Preferences has an on/off switch, a speed control and, for each of the cap's six axes, an
invert switch and a speed for the 3D mouse. The web app reads
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
