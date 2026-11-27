/// Mouse button (or similar for touch input)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum PointerButton {
    /// The primary mouse button is usually the left one.
    Primary = 0,

    /// The secondary mouse button is usually the right one,
    /// and most often used for context menus or other optional things.
    Secondary = 1,

    /// The tertiary mouse button is usually the middle mouse button (e.g. clicking the scroll wheel).
    Middle = 2,

    /// The first extra mouse button on some mice. In web typically corresponds to the Browser back button.
    Extra1 = 3,

    /// The second extra mouse button on some mice. In web typically corresponds to the Browser forward button.
    Extra2 = 4,

    // SolidCraft: the further buttons of mice with many buttons (button 6 to 16, see
    // [`PointerButton::number`]). The platform reports them as `winit::event::MouseButton::Other`.
    /// Mouse button 6.
    Extra3 = 5,
    /// Mouse button 7.
    Extra4 = 6,
    /// Mouse button 8.
    Extra5 = 7,
    /// Mouse button 9.
    Extra6 = 8,
    /// Mouse button 10.
    Extra7 = 9,
    /// Mouse button 11.
    Extra8 = 10,
    /// Mouse button 12.
    Extra9 = 11,
    /// Mouse button 13.
    Extra10 = 12,
    /// Mouse button 14.
    Extra11 = 13,
    /// Mouse button 15.
    Extra12 = 14,
    /// Mouse button 16.
    Extra13 = 15,
}

/// Number of pointer buttons supported by egui, i.e. the number of possible states of [`PointerButton`].
// SolidCraft: 16 (was 5), for the further buttons.
pub const NUM_POINTER_BUTTONS: usize = 16;

// SolidCraft: every button in order, and buttons by the number people count them by.
impl PointerButton {
    /// Every button, in the order of their numbers.
    pub const ALL: [Self; NUM_POINTER_BUTTONS] = [
        Self::Primary,
        Self::Secondary,
        Self::Middle,
        Self::Extra1,
        Self::Extra2,
        Self::Extra3,
        Self::Extra4,
        Self::Extra5,
        Self::Extra6,
        Self::Extra7,
        Self::Extra8,
        Self::Extra9,
        Self::Extra10,
        Self::Extra11,
        Self::Extra12,
        Self::Extra13,
    ];

    /// The button's number: 1 primary, 2 secondary, 3 middle, 4 and 5 the two side buttons
    /// ([`Self::Extra1`], [`Self::Extra2`]), 6 to 16 the further ones.
    #[inline]
    pub fn number(self) -> u16 {
        self as u16 + 1
    }

    /// The button numbered `number` (see [`Self::number`]), if egui has one.
    pub fn from_number(number: u16) -> Option<Self> {
        Self::ALL.get(usize::from(number.checked_sub(1)?)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{NUM_POINTER_BUTTONS, PointerButton};

    #[test]
    fn buttons_are_numbered_in_order() {
        for (i, button) in PointerButton::ALL.into_iter().enumerate() {
            assert_eq!(button as usize, i);
            assert_eq!(PointerButton::from_number(button.number()), Some(button));
        }
        assert_eq!(PointerButton::from_number(0), None);
        assert_eq!(PointerButton::from_number(NUM_POINTER_BUTTONS as u16 + 1), None);
        assert_eq!(PointerButton::Extra1.number(), 4);
        assert_eq!(PointerButton::Extra3.number(), 6);
    }
}
