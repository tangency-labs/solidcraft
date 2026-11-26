//! A set of button numbers. A HID 3D mouse numbers its buttons from 1 in the order its descriptor
//! declares them, and those numbers are 3Dconnexion's button ids; the SpaceMouse Enterprise's
//! reach 176 (Space), past what a `u32` bitmask holds.

/// Button numbers 1 to 255; 0 is never a button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ButtonSet([u64; 4]);

impl ButtonSet {
    pub const EMPTY: Self = Self([0; 4]);

    /// The buttons of a bitmask with bit N for button N + 1, as [`crate::Report::buttons`] has them.
    pub fn from_mask(mask: u32) -> Self {
        Self([u64::from(mask) << 1, 0, 0, 0])
    }

    /// The set holding only `number`.
    pub fn of(number: u8) -> Self {
        let mut set = Self::EMPTY;
        set.insert(number);
        set
    }

    fn slot(number: u8) -> (usize, u64) {
        (usize::from(number / 64), 1_u64 << (number % 64))
    }

    /// Adds `number`; 0 is ignored.
    pub fn insert(&mut self, number: u8) {
        if number != 0 {
            let (word, bit) = Self::slot(number);
            self.0[word] |= bit;
        }
    }

    pub fn remove(&mut self, number: u8) {
        let (word, bit) = Self::slot(number);
        self.0[word] &= !bit;
    }

    pub fn contains(self, number: u8) -> bool {
        let (word, bit) = Self::slot(number);
        number != 0 && self.0[word] & bit != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }

    /// The buttons in `self` that are not in `other`.
    pub fn minus(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] & !other.0[i]))
    }

    /// The button numbers, lowest first.
    pub fn iter(self) -> impl Iterator<Item = u8> {
        (1..=u8::MAX).filter(move |&number| self.contains(number))
    }
}

impl std::ops::BitOr for ButtonSet {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] | other.0[i]))
    }
}

impl std::ops::BitOrAssign for ButtonSet {
    fn bitor_assign(&mut self, other: Self) {
        *self = *self | other;
    }
}

impl std::ops::BitAnd for ButtonSet {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] & other.0[i]))
    }
}

impl FromIterator<u8> for ButtonSet {
    fn from_iter<I: IntoIterator<Item = u8>>(numbers: I) -> Self {
        let mut set = Self::EMPTY;
        for number in numbers {
            set.insert(number);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A HID bitmask keeps its numbering: bit N is button N + 1, so bit 31 is button 32.
    #[test]
    fn a_mask_maps_bit_n_to_button_n_plus_one() {
        let set = ButtonSet::from_mask(0b101 | 1 << 31);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![1, 3, 32]);
        assert!(ButtonSet::from_mask(0).is_empty());
    }

    /// The SpaceMouse Enterprise's ids go past 32: Tab is 175, Space 176, and 255 is the last.
    #[test]
    fn numbers_past_32_up_to_255_fit() {
        let mut set: ButtonSet = [3, 64, 175, 176, 255].into_iter().collect();
        assert!(set.contains(176) && set.contains(64) && set.contains(255));
        assert!(!set.contains(177) && !set.contains(63));
        set.remove(176);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![3, 64, 175, 255]);
    }

    /// 0 is no button: inserting it does nothing and it is never contained.
    #[test]
    fn zero_is_never_a_button() {
        let set = ButtonSet::of(0);
        assert!(set.is_empty());
        assert!(!set.contains(0));
    }

    #[test]
    fn union_intersection_and_difference_work_across_words() {
        let a: ButtonSet = [1, 100, 200].into_iter().collect();
        let b: ButtonSet = [100, 150].into_iter().collect();
        assert_eq!((a | b).iter().collect::<Vec<_>>(), vec![1, 100, 150, 200]);
        assert_eq!((a & b).iter().collect::<Vec<_>>(), vec![100]);
        assert_eq!(a.minus(b).iter().collect::<Vec<_>>(), vec![1, 200]);
    }
}
