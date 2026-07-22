#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::mem::size_of;
use std::num::NonZeroU32;

use super::*;

#[test]
fn text_offsets_are_fixed_width_and_checked() {
    assert_eq!(size_of::<TextSize>(), size_of::<u32>());
    assert_eq!(
        TextSize::try_from(usize::try_from(u32::MAX).unwrap()).unwrap(),
        TextSize::MAX
    );

    if usize::BITS > u32::BITS {
        let overflow = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
        let error = TextSize::try_from(overflow).unwrap_err();
        assert_eq!(error.value(), overflow);
    }
}

#[test]
fn one_based_physical_coordinates_reject_zero_and_overflow() {
    assert_eq!(
        PhysicalLineColumn::try_from_usize(0, 1).unwrap_err(),
        PhysicalLineColumnOverflow::Line { value: 0 }
    );
    assert_eq!(
        PhysicalLineColumn::try_from_usize(1, 0).unwrap_err(),
        PhysicalLineColumnOverflow::ByteColumn { value: 0 }
    );

    if usize::BITS > u32::BITS {
        let overflow = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
        assert_eq!(
            PhysicalLineColumn::try_from_usize(overflow, 1).unwrap_err(),
            PhysicalLineColumnOverflow::Line { value: overflow }
        );
        assert_eq!(
            PhysicalLineColumn::try_from_usize(1, overflow).unwrap_err(),
            PhysicalLineColumnOverflow::ByteColumn { value: overflow }
        );
    }
}

#[test]
fn text_ranges_are_checked_and_half_open() {
    let range = TextRange::new(TextSize::new(2), TextSize::new(5)).unwrap();
    assert_eq!(range.start(), TextSize::new(2));
    assert_eq!(range.end(), TextSize::new(5));
    assert_eq!(range.len(), TextSize::new(3));
    assert!(range.contains(TextSize::new(2)));
    assert!(range.contains(TextSize::new(4)));
    assert!(!range.contains(TextSize::new(5)));

    let empty = TextRange::new(TextSize::new(5), TextSize::new(5)).unwrap();
    assert!(empty.is_empty());
    assert!(!empty.contains(TextSize::new(5)));

    let invalid = TextRange::new(TextSize::new(6), TextSize::new(5)).unwrap_err();
    assert_eq!(invalid.start(), TextSize::new(6));
    assert_eq!(invalid.end(), TextSize::new(5));
}

#[test]
fn logical_column_distinguishes_hidden_from_first_column() {
    let hidden = LogicalColumn::from_go_column(0);
    let first = LogicalColumn::from_go_column(1);

    assert_eq!(hidden, LogicalColumn::Hidden);
    assert_eq!(hidden.to_go_column(), 0);
    assert_eq!(hidden.known(), None);
    assert_eq!(first, LogicalColumn::Known(NonZeroU32::new(1).unwrap()));
    assert_eq!(first.to_go_column(), 1);
    assert_eq!(first.known(), NonZeroU32::new(1));

    let adjusted = LogicalLineColumn::new(NonZeroU32::new(40).unwrap(), hidden);
    assert_eq!(adjusted.line().get(), 40);
    assert_eq!(adjusted.column(), LogicalColumn::Hidden);
}
