# solidcraft-space-mouse

3D mouse input that doesn't depend on how the device is opened or what the motion drives. A host
opens the device its own way (SolidCraft uses hidapi on Windows and macOS and hidraw on Linux),
passes the device's HID report descriptor to `Descriptor::parse` and each input report to
`Layout::decode`, and applies the six axes and the buttons it gets back to its own camera and
commands.

Any device that declares a HID Multi-axis Controller is read the same way: 3Dconnexion's, which send
translation and rotation in two reports and their buttons in a third, and homemade ones, which often
send all six axes in one report. The descriptor says where each axis and button is.

The crate has no dependencies (`serde` is an optional feature, for saving a calibration with a
host's preferences) and no `unsafe`. It is shared so that every host reads devices the same way;
hosts that draw with the UI kit reach it as `solidcraft_ui_kit::space_mouse`.

## API

| Item | What it is |
|---|---|
| `Descriptor::parse` | A device's HID report descriptor, read into where each axis and button sits |
| `Layout::decode` | One input report, read into axes or buttons (`Report`) |
| `ButtonSet` | The buttons that are down, by 3Dconnexion button id |
| `buttons` | What each button does by default, for the devices measured |
| `Calibration`, `AxisCalibration`, `Wizard` | Each axis's rest point, dead zone and range, and the wizard that measures them |
| `Settle` | Once a frame, holds an axis at rest while the cap settles back after a move |
