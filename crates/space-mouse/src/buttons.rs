//! What each button on a 3D mouse does by default, for the devices we have measured. The jobs
//! come from 3Dconnexion's public manuals (SpaceMouse Pro Feature Guide, SpaceMouse Enterprise
//! user manual); the button numbers come only from our own measurements. Nothing here is guessed:
//! a device or button that isn't in [`MEASURED`] has no default.
//!
//! Button numbers are 3Dconnexion's button ids, which the HID devices report as their button
//! numbers and drivers such as smed report as ids; they run past 32 (the Enterprise's Space is 176).

use std::collections::BTreeMap;

/// A job a button can do by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Opens the device's settings.
    Menu,
    /// Brings the model back to the centre of the screen.
    Fit,
    ViewTop,
    ViewRight,
    ViewFront,
    ViewBottom,
    ViewLeft,
    ViewBack,
    /// The isometric view.
    ViewIso,
    /// Rolls the view.
    Roll,
    Esc,
    Alt,
    Shift,
    Ctrl,
    Enter,
    Delete,
    Tab,
    Space,
    /// Locks rotation with one press.
    RotationLock,
}

/// Whether a row is about a brief press or a press held past the device's long-press threshold.
/// Only devices that tell the two apart (the SpaceMouse Enterprise, read through a driver that
/// reports its long presses) have held pickers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Short,
    Long,
}

/// One measured button: device (USB vendor and product id), the button number the device
/// reports (its 3Dconnexion id, counting from 1 like [`crate::Layout::buttons`]), short or long
/// press, and the job it does by default. `None` means the key is measured but has no fixed
/// default (the numbered function keys, whose commands the vendor's driver assigns per
/// application).
type Row = (u16, u16, usize, Hold, Option<Action>);

use Hold::{Long, Short};

/// The only place button numbers live. To add a key, add a row from a measurement; don't
/// guess a number from memory or from anyone else's code.
const MEASURED: &[Row] = &[
    // SpaceMouse Pro (046d:c62b): our own measurement on a SpaceMouse Pro, 2026-10-16.
    // Each row is one printed key.
    (0x046d, 0xc62b, 1, Short, Some(Action::Menu)),
    (0x046d, 0xc62b, 2, Short, Some(Action::Fit)),
    (0x046d, 0xc62b, 3, Short, Some(Action::ViewTop)),
    (0x046d, 0xc62b, 5, Short, Some(Action::ViewRight)),
    (0x046d, 0xc62b, 6, Short, Some(Action::ViewFront)),
    (0x046d, 0xc62b, 9, Short, Some(Action::Roll)),
    (0x046d, 0xc62b, 13, Short, None), // printed "1"
    (0x046d, 0xc62b, 14, Short, None), // printed "2"
    (0x046d, 0xc62b, 15, Short, None), // printed "3"
    (0x046d, 0xc62b, 16, Short, None), // printed "4"
    (0x046d, 0xc62b, 23, Short, Some(Action::Esc)),
    (0x046d, 0xc62b, 24, Short, Some(Action::Alt)),
    (0x046d, 0xc62b, 25, Short, Some(Action::Shift)),
    (0x046d, 0xc62b, 26, Short, Some(Action::Ctrl)),
    (0x046d, 0xc62b, 27, Short, Some(Action::RotationLock)),
    // SpaceMouse Enterprise (256f:c633): our own measurement on a SpaceMouse Enterprise,
    // 2026-11-13 (all 31 keys, short and long, read from its HID reports 0x1c and 0x1d).
    // Each row is one printed key; a long press reports the key's own id.
    (0x256f, 0xc633, 1, Short, Some(Action::Menu)),
    (0x256f, 0xc633, 2, Short, Some(Action::Fit)),
    (0x256f, 0xc633, 3, Short, Some(Action::ViewTop)),
    (0x256f, 0xc633, 3, Long, Some(Action::ViewBottom)),
    (0x256f, 0xc633, 5, Short, Some(Action::ViewRight)),
    (0x256f, 0xc633, 5, Long, Some(Action::ViewLeft)),
    (0x256f, 0xc633, 6, Short, Some(Action::ViewFront)),
    (0x256f, 0xc633, 6, Long, Some(Action::ViewBack)),
    (0x256f, 0xc633, 9, Short, Some(Action::Roll)),
    (0x256f, 0xc633, 11, Short, Some(Action::ViewIso)), // printed "ISO"
    (0x256f, 0xc633, 13, Short, None),                  // printed "1"; 14 to 22 are "2" to "10"
    (0x256f, 0xc633, 14, Short, None),
    (0x256f, 0xc633, 15, Short, None),
    (0x256f, 0xc633, 16, Short, None),
    (0x256f, 0xc633, 17, Short, None),
    (0x256f, 0xc633, 18, Short, None),
    (0x256f, 0xc633, 19, Short, None),
    (0x256f, 0xc633, 20, Short, None),
    (0x256f, 0xc633, 21, Short, None),
    (0x256f, 0xc633, 22, Short, None),
    (0x256f, 0xc633, 23, Short, Some(Action::Esc)),
    (0x256f, 0xc633, 24, Short, Some(Action::Alt)),
    (0x256f, 0xc633, 25, Short, Some(Action::Shift)),
    (0x256f, 0xc633, 26, Short, Some(Action::Ctrl)),
    (0x256f, 0xc633, 27, Short, Some(Action::RotationLock)),
    (0x256f, 0xc633, 36, Short, Some(Action::Enter)),
    (0x256f, 0xc633, 37, Short, Some(Action::Delete)),
    (0x256f, 0xc633, 77, Short, None),  // printed "11"
    (0x256f, 0xc633, 78, Short, None),  // printed "12"
    (0x256f, 0xc633, 103, Short, None), // printed "V1"; 104, 105 are "V2", "V3"
    (0x256f, 0xc633, 104, Short, None),
    (0x256f, 0xc633, 105, Short, None),
    (0x256f, 0xc633, 175, Short, Some(Action::Tab)),
    (0x256f, 0xc633, 176, Short, Some(Action::Space)),
];

/// What is printed on a measured key: its text (`T`, `Esc`, `V1`), or a symbol, which the interface
/// names in the user's language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Printed {
    Text(&'static str),
    /// The roll arrow (rolls the view by 90°).
    Roll,
    /// The padlock (the key that switches the rotation lock).
    Lock,
}

use Printed::Text;

/// What is printed on each measured key, to name it in the interface instead of its number. Same
/// measurements as [`MEASURED`].
const PRINTED: &[(u16, u16, u8, Printed)] = &[
    // SpaceMouse Pro (046d:c62b)
    (0x046d, 0xc62b, 1, Text("Menu")),
    (0x046d, 0xc62b, 2, Text("Fit")),
    (0x046d, 0xc62b, 3, Text("T")),
    (0x046d, 0xc62b, 5, Text("R")),
    (0x046d, 0xc62b, 6, Text("F")),
    (0x046d, 0xc62b, 9, Printed::Roll),
    (0x046d, 0xc62b, 13, Text("1")),
    (0x046d, 0xc62b, 14, Text("2")),
    (0x046d, 0xc62b, 15, Text("3")),
    (0x046d, 0xc62b, 16, Text("4")),
    (0x046d, 0xc62b, 23, Text("Esc")),
    (0x046d, 0xc62b, 24, Text("Alt")),
    (0x046d, 0xc62b, 25, Text("Shift")),
    (0x046d, 0xc62b, 26, Text("Ctrl")),
    (0x046d, 0xc62b, 27, Printed::Lock),
    // SpaceMouse Enterprise (256f:c633)
    (0x256f, 0xc633, 1, Text("Menu")),
    (0x256f, 0xc633, 2, Text("Fit")),
    (0x256f, 0xc633, 3, Text("T")),
    (0x256f, 0xc633, 5, Text("R")),
    (0x256f, 0xc633, 6, Text("F")),
    (0x256f, 0xc633, 9, Printed::Roll),
    (0x256f, 0xc633, 11, Text("ISO")),
    (0x256f, 0xc633, 13, Text("1")),
    (0x256f, 0xc633, 14, Text("2")),
    (0x256f, 0xc633, 15, Text("3")),
    (0x256f, 0xc633, 16, Text("4")),
    (0x256f, 0xc633, 17, Text("5")),
    (0x256f, 0xc633, 18, Text("6")),
    (0x256f, 0xc633, 19, Text("7")),
    (0x256f, 0xc633, 20, Text("8")),
    (0x256f, 0xc633, 21, Text("9")),
    (0x256f, 0xc633, 22, Text("10")),
    (0x256f, 0xc633, 23, Text("Esc")),
    (0x256f, 0xc633, 24, Text("Alt")),
    (0x256f, 0xc633, 25, Text("Shift")),
    (0x256f, 0xc633, 26, Text("Ctrl")),
    (0x256f, 0xc633, 27, Printed::Lock),
    (0x256f, 0xc633, 36, Text("Enter")),
    (0x256f, 0xc633, 37, Text("Delete")),
    (0x256f, 0xc633, 77, Text("11")),
    (0x256f, 0xc633, 78, Text("12")),
    (0x256f, 0xc633, 103, Text("V1")),
    (0x256f, 0xc633, 104, Text("V2")),
    (0x256f, 0xc633, 105, Text("V3")),
    (0x256f, 0xc633, 175, Text("Tab")),
    (0x256f, 0xc633, 176, Text("Space")),
];

/// What is printed on button `number` of this device, or `None` for a key we have not measured.
pub fn printed(vendor: u16, product: u16, number: u8) -> Option<Printed> {
    PRINTED.iter().find(|row| (row.0, row.1, row.2) == (vendor, product, number)).map(|row| row.3)
}

/// The highest button number a row may have.
const MAX_NUMBER: usize = u8::MAX as usize;

/// The default job of each button on this device for a brief press, by button number. `None` for
/// a device we haven't measured, including homemade ones. A measured button without a default
/// is not in the map.
pub fn defaults(vendor: u16, product: u16) -> Option<BTreeMap<u8, Action>> {
    from_rows(MEASURED, vendor, product, Short)
}

/// The default job of each button on this device for a long press, by button number; `None`
/// for a device that has no long-press defaults.
pub fn long_defaults(vendor: u16, product: u16) -> Option<BTreeMap<u8, Action>> {
    from_rows(MEASURED, vendor, product, Long)
}

fn from_rows(rows: &[Row], vendor: u16, product: u16, hold: Hold) -> Option<BTreeMap<u8, Action>> {
    let out: BTreeMap<u8, Action> = rows
        .iter()
        .filter(|row| row.0 == vendor && row.1 == product && row.3 == hold)
        // A row with a number outside 1..=255 is ignored rather than trusted.
        .filter(|row| (1..=MAX_NUMBER).contains(&row.2))
        .filter_map(|&(_, _, number, _, action)| Some((u8::try_from(number).ok()?, action?)))
        .collect();
    // A device whose rows give no action has no defaults at all.
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_rows_resolve() {
        let table = defaults(0x046d, 0xc62b).expect("measured device");
        let at = |n: u8| table.get(&n).copied();
        assert_eq!(at(1), Some(Action::Menu));
        assert_eq!(at(2), Some(Action::Fit));
        assert_eq!(at(3), Some(Action::ViewTop));
        assert_eq!(at(5), Some(Action::ViewRight));
        assert_eq!(at(6), Some(Action::ViewFront));
        assert_eq!(at(9), Some(Action::Roll));
        assert_eq!(at(23), Some(Action::Esc));
        assert_eq!(at(24), Some(Action::Alt));
        assert_eq!(at(25), Some(Action::Shift));
        assert_eq!(at(26), Some(Action::Ctrl));
        assert_eq!(at(27), Some(Action::RotationLock));
        // The numbered keys 1 to 4, undeclared numbers and the unused tail have no action.
        for n in [4, 7, 8, 10, 13, 14, 15, 16, 28, 32] {
            assert_eq!(at(n), None);
        }
        assert!(long_defaults(0x046d, 0xc62b).is_none(), "the Pro reports no long presses");
    }

    /// The Enterprise's keys past 32 resolve, and its long presses give the opposite views.
    #[test]
    fn enterprise_rows_resolve_short_and_long() {
        let short = defaults(0x256f, 0xc633).expect("measured device");
        assert_eq!(short.get(&11), Some(&Action::ViewIso));
        assert_eq!(short.get(&36), Some(&Action::Enter));
        assert_eq!(short.get(&37), Some(&Action::Delete));
        assert_eq!(short.get(&175), Some(&Action::Tab));
        assert_eq!(short.get(&176), Some(&Action::Space));
        for n in [13, 22, 77, 78, 103, 105] {
            assert_eq!(short.get(&n), None, "button {n} has no fixed default");
        }
        let long = long_defaults(0x256f, 0xc633).expect("long presses measured");
        assert_eq!(long.get(&3), Some(&Action::ViewBottom));
        assert_eq!(long.get(&5), Some(&Action::ViewLeft));
        assert_eq!(long.get(&6), Some(&Action::ViewBack));
        assert_eq!(long.len(), 3);
    }

    #[test]
    fn rows_resolve_to_their_action() {
        let rows: &[Row] = &[
            // Synthetic numbers, not real mappings.
            (1, 2, 5, Short, Some(Action::Fit)),
            // Synthetic numbers, not real mappings.
            (1, 2, 1, Short, Some(Action::Menu)),
            // Synthetic numbers, not real mappings: zero and past 255 are dropped.
            (1, 2, 0, Short, Some(Action::Esc)),
            // Synthetic numbers, not real mappings.
            (1, 2, MAX_NUMBER + 1, Short, Some(Action::Alt)),
            // Synthetic numbers, not real mappings: a long row is not a short default.
            (1, 2, 7, Long, Some(Action::Ctrl)),
            // Synthetic numbers, not real mappings: a device with only no-action rows.
            (1, 4, 7, Short, None),
        ];
        let table = from_rows(rows, 1, 2, Short).expect("known device");
        assert_eq!(table.get(&5), Some(&Action::Fit));
        assert_eq!(table.get(&1), Some(&Action::Menu));
        assert_eq!(table.len(), 2);
        assert_eq!(from_rows(rows, 1, 2, Long).map(|t| t.len()), Some(1));
        assert!(from_rows(rows, 1, 3, Short).is_none());
        assert!(from_rows(rows, 1, 4, Short).is_none());
    }

    /// Every measured key of the Enterprise has its printed name, and only measured keys have one.
    #[test]
    fn printed_names_cover_the_measured_keys() {
        assert_eq!(printed(0x256f, 0xc633, 3), Some(Text("T")));
        assert_eq!(printed(0x256f, 0xc633, 13), Some(Text("1")));
        assert_eq!(printed(0x256f, 0xc633, 78), Some(Text("12")));
        assert_eq!(printed(0x256f, 0xc633, 176), Some(Text("Space")));
        assert_eq!(printed(0x256f, 0xc633, 9), Some(Printed::Roll));
        assert_eq!(printed(0x046d, 0xc62b, 27), Some(Printed::Lock));
        assert_eq!(printed(0x256f, 0xc633, 4), None);
        assert_eq!(printed(0, 0, 1), None);
        for &(vendor, product, number, _, _) in MEASURED {
            let number = u8::try_from(number).unwrap();
            assert!(printed(vendor, product, number).is_some(), "button {number} of {vendor:04x}:{product:04x} has no name");
        }
        for &(vendor, product, number, _) in PRINTED {
            assert!(MEASURED.iter().any(|row| (row.0, row.1, row.2) == (vendor, product, usize::from(number))), "{number} is not measured");
        }
    }

    #[test]
    fn measured_table_has_no_duplicates() {
        for (i, a) in MEASURED.iter().enumerate() {
            for b in MEASURED.iter().skip(i + 1) {
                assert!((a.0, a.1, a.2, a.3) != (b.0, b.1, b.2, b.3), "duplicate row for button {}", a.2);
            }
        }
    }

    #[test]
    fn unknown_devices_have_no_defaults() {
        assert!(defaults(0x046d, 0xc62a).is_none());
        assert!(defaults(0x256f, 0xc63a).is_none());
        assert!(defaults(0, 0).is_none());
        assert!(defaults(u16::MAX, u16::MAX).is_none());
        assert!(long_defaults(0, 0).is_none());
    }

    #[test]
    fn measured_table_stays_in_range() {
        for &(_, _, number, _, _) in MEASURED {
            assert!((1..=MAX_NUMBER).contains(&number));
        }
        // Whatever the ids, lookups never panic.
        for v in [0, 1, 0x046d, 0x256f, u16::MAX] {
            for p in [0, 0xc62b, 0xc633, u16::MAX] {
                let _ = defaults(v, p);
                let _ = long_defaults(v, p);
            }
        }
    }
}
