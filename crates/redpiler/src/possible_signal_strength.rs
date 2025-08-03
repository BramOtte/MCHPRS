/// A bitset of the possible output signal strengths with 1 << N representing a signal strength of N
#[derive(Debug, Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PossibleSS(u16);

impl PossibleSS {
    pub const POSITIVE: Self = Self(0xffff << 1);
    pub const BOOL: Self = Self::from_slice(&[0, 15]);
    pub const FULL: Self = Self(0xffff);
    pub const EMPTY: Self = Self(0);

    #[inline]
    pub const fn new(bitset: u16) -> Self {
        Self(bitset)
    }

    #[inline]
    pub const fn constant(ss: u8) -> Self {
        debug_assert!(ss <= 15);
        Self(1 << ss)
    }

    #[inline]
    pub const fn from_range(range: std::ops::RangeInclusive<u8>) -> Self {
        debug_assert!(*range.end() <= 15);
        debug_assert!(*range.start() <= *range.end());
        Self(0xffffu16 >> (15u8 + *range.start() - *range.end()) << *range.start())
    }

    #[inline]
    pub const fn from_slice(arr: &[u8]) -> Self {
        let mut bitset = 0;
        let mut i = 0;
        while i < arr.len() {
            let ss = arr[i];
            debug_assert!(ss <= 15);
            bitset |= 1 << ss;
            i += 1;
        }
        Self(bitset)
    }

    #[inline]
    pub const fn size(self) -> u8 {
        self.0.count_ones() as u8
    }

    #[inline]
    pub const fn with(self, ss: u8) -> Self {
        debug_assert!(ss <= 15);
        Self(self.0 | (1 << ss))
    }

    #[inline]
    pub const fn insert(&mut self, ss: u8) {
        debug_assert!(ss <= 15);
        self.0 |= 1 << ss
    }

    #[inline]
    pub const fn union(&mut self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn intersect(&mut self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[inline]
    pub const fn contains(self, ss: u8) -> bool {
        debug_assert!(ss <= 15);
        self.0 & (1 << ss) != 0
    }

    #[inline]
    pub const fn contains_any(self, set: PossibleSS) -> bool {
        self.0 & set.0 != 0
    }

    #[inline]
    pub const fn contains_all(self, set: PossibleSS) -> bool {
        self.0 & set.0 == set.0
    }

    #[inline]
    pub const fn contains_positive(self) -> bool {
        self.0 & Self::POSITIVE.0 != 0
    }

    #[inline]
    pub const fn min_ss(self) -> u8 {
        (self.0.trailing_zeros() as u8) & 15
    }

    #[inline]
    pub const fn max_ss(self) -> u8 {
        if let Some(ss) = self.0.checked_ilog2() {
            ss as u8
        } else {
            0
        }
    }

    #[inline]
    pub const fn dust_or(self, other: Self) -> Self {
        Self(dust_or(self.0, other.0))
    }

    #[inline]
    pub const fn subtract_ss(self, distance: u8) -> Self {
        Self((self.0 & 1) | (self.0 >> distance))
    }

    #[inline]
    pub const fn bool_signature(self, ss_dist: u8) -> u16 {
        self.0 & (Self::POSITIVE.0 << ss_dist)
    }

    #[inline]
    pub const fn hex_signature(self, ss_dist: u8) -> u16 {
        (self.0 & 1) | ((self.0 & Self::POSITIVE.0) >> ss_dist)
    }

    #[inline]
    pub const fn is_constant(self) -> bool {
        self.0.count_ones() <= 1
    }

    #[inline]
    pub const fn get_constant(self) -> Option<u8> {
        if self.is_constant() {
            Some(self.max_ss())
        } else {
            None
        }
    }

    pub const fn insert_zero_if_empty(&mut self) {
        self.0 = if self.0 == 0 { 1 } else { self.0 };
    }
}

#[inline(always)]
const fn dust_or(a: u16, b: u16) -> u16 {
    let a_lsb = a & (0u16.wrapping_sub(a));
    let a_mask = !a_lsb.saturating_sub(1);

    let b_lsb = b & (0u16.wrapping_sub(b));
    let b_mask = !b_lsb.saturating_sub(1);

    (a | b) & a_mask & b_mask
}

#[test]
fn test_dust_or() {
    assert_eq!(dust_or(0b1010100, 0b1010), 0b1011100);
    assert_eq!(dust_or(0b1010, 0b1010100), 0b1011100);

    assert_eq!(dust_or(0b111010100, 0b0), 0b111010100);
    assert_eq!(dust_or(0b0, 0b111010100), 0b111010100);
}

#[test]
fn test_min_max_ss() {
    assert_eq!(PossibleSS::from_slice(&[1, 2, 3]).min_ss(), 1);
    assert_eq!(PossibleSS::from_slice(&[1, 2, 3]).max_ss(), 3);

    assert_eq!(PossibleSS::from_slice(&[2, 3, 7, 15]).max_ss(), 15);
    assert_eq!(PossibleSS::from_slice(&[2, 3, 7, 15]).min_ss(), 2);

    // incase there are no inputs the signal strength will always be 0
    assert_eq!(PossibleSS::from_slice(&[]).min_ss(), 0);
    assert_eq!(PossibleSS::from_slice(&[]).max_ss(), 0);
}

#[test]
fn test_from_range() {
    assert_eq!(PossibleSS::from_range(0..=0), PossibleSS::constant(0));
    assert_eq!(PossibleSS::from_range(7..=7), PossibleSS::constant(7));
    assert_eq!(
        PossibleSS::from_range(7..=8),
        PossibleSS::from_slice(&[7, 8])
    );
    assert_eq!(PossibleSS::from_range(15..=15), PossibleSS::constant(15));
    assert_eq!(PossibleSS::from_range(0..=15), PossibleSS::FULL);
    assert_eq!(
        PossibleSS::from_range(0..=3),
        PossibleSS::from_slice(&[0, 1, 2, 3])
    );
    assert_eq!(
        PossibleSS::from_range(5..=15),
        PossibleSS::from_slice(&[5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
    );
    assert_eq!(
        PossibleSS::from_range(1..=14),
        PossibleSS::from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14])
    );
    assert_eq!(
        PossibleSS::from_range(2..=13),
        PossibleSS::from_slice(&[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13])
    );
}
