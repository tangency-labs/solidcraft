//! What each button on a 3D mouse does by default, for the devices we have measured. The jobs
//! come from 3Dconnexion's public SpaceMouse Pro manual (Feature Guide); the button numbers come
//! only from our own measurements. Nothing here is guessed: a device or button that isn't in
//! [`MEASURED`] has no default.

use crate::MAX_BUTTONS;

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
    /// Rolls the view.
    Roll,
    Esc,
    Alt,
    Shift,
    Ctrl,
    /// Locks rotation with one press.
    RotationLock,
}

/// One measured button: device (USB vendor and product id), the button number the device
/// reports (counting from 1, like [`crate::Layout::buttons`]), and the job it does by default.
/// `None` means the key is measured but has no fixed default (the numbered function keys, whose
/// commands the vendor's driver assigns per application).
type Row = (u16, u16, usize, Option<Action>);

/// The only place button numbers live. To add a key, add a row from a measurement; don't
/// guess a number from memory or from anyone else's code.
const MEASURED: &[Row] = &[
    // SpaceMouse Pro (046d:c62b): our own measurement on a SpaceMouse Pro, 2026-10-16.
    // Each row is one printed key.
    (0x046d, 0xc62b, 1, Some(Action::Menu)),
    (0x046d, 0xc62b, 2, Some(Action::Fit)),
    (0x046d, 0xc62b, 3, Some(Action::ViewTop)),
    (0x046d, 0xc62b, 5, Some(Action::ViewRight)),
    (0x046d, 0xc62b, 6, Some(Action::ViewFront)),
    (0x046d, 0xc62b, 9, Some(Action::Roll)),
    (0x046d, 0xc62b, 13, None), // printed "1"
    (0x046d, 0xc62b, 14, None), // printed "2"
    (0x046d, 0xc62b, 15, None), // printed "3"
    (0x046d, 0xc62b, 16, None), // printed "4"
    (0x046d, 0xc62b, 23, Some(Action::Esc)),
    (0x046d, 0xc62b, 24, Some(Action::Alt)),
    (0x046d, 0xc62b, 25, Some(Action::Shift)),
    (0x046d, 0xc62b, 26, Some(Action::Ctrl)),
    (0x046d, 0xc62b, 27, Some(Action::RotationLock)),
];

/// The default job of each button on this device, indexed like [`crate::Layout::buttons`] (button
/// number minus 1). `None` for a device we haven't measured, including homemade ones. For a
/// measured device, a button's entry is `None` unless we've measured it and it has a default.
pub fn defaults(vendor: u16, product: u16) -> Option<[Option<Action>; MAX_BUTTONS]> {
    from_rows(MEASURED, vendor, product)
}

fn from_rows(rows: &[Row], vendor: u16, product: u16) -> Option<[Option<Action>; MAX_BUTTONS]> {
    let mut rows = rows.iter().filter(|row| row.0 == vendor && row.1 == product).peekable();
    rows.peek()?;
    let mut out = [None; MAX_BUTTONS];
    for &(_, _, number, action) in rows {
        // A row with a number outside 1..=MAX_BUTTONS is ignored rather than trusted.
        if let Some(slot) = number.checked_sub(1).and_then(|i| out.get_mut(i)) {
            *slot = action;
        }
    }
    // A device whose rows give no action has no defaults at all.
    out.iter().any(Option::is_some).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_rows_resolve() {
        let table = defaults(0x046d, 0xc62b).expect("measured device");
        let at = |n: usize| table.get(n - 1).copied().flatten();
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
    }

    #[test]
    fn rows_resolve_to_their_action() {
        let rows: &[Row] = &[
            // Synthetic numbers, not real mappings.
            (1, 2, 5, Some(Action::Fit)),
            // Synthetic numbers, not real mappings.
            (1, 2, 1, Some(Action::Menu)),
            // Synthetic numbers, not real mappings: zero and past MAX_BUTTONS are dropped.
            (1, 2, 0, Some(Action::Esc)),
            // Synthetic numbers, not real mappings.
            (1, 2, MAX_BUTTONS + 1, Some(Action::Alt)),
            // Synthetic numbers, not real mappings: a device with only no-action rows.
            (1, 4, 7, None),
        ];
        let table = from_rows(rows, 1, 2).expect("known device");
        assert_eq!(table.get(4), Some(&Some(Action::Fit)));
        assert_eq!(table.first(), Some(&Some(Action::Menu)));
        assert_eq!(table.iter().flatten().count(), 2);
        assert!(from_rows(rows, 1, 3).is_none());
        assert!(from_rows(rows, 1, 4).is_none());
    }

    #[test]
    fn measured_table_has_no_duplicates() {
        for (i, a) in MEASURED.iter().enumerate() {
            for b in MEASURED.iter().skip(i + 1) {
                assert!((a.0, a.1, a.2) != (b.0, b.1, b.2), "duplicate row for button {}", a.2);
            }
        }
    }

    #[test]
    fn unknown_devices_have_no_defaults() {
        assert!(defaults(0x046d, 0xc62a).is_none());
        assert!(defaults(0x256f, 0xc63a).is_none());
        assert!(defaults(0, 0).is_none());
        assert!(defaults(u16::MAX, u16::MAX).is_none());
    }

    #[test]
    fn measured_table_stays_in_range() {
        for &(_, _, number, _) in MEASURED {
            assert!((1..=MAX_BUTTONS).contains(&number));
        }
        // Whatever the ids, lookups never panic.
        for v in [0, 1, 0x046d, u16::MAX] {
            for p in [0, 0xc62b, u16::MAX] {
                let _ = defaults(v, p);
            }
        }
    }
}
