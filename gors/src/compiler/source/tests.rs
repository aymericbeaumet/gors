#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::mem::size_of;
use std::num::NonZeroU32;

use super::*;
use crate::compiler::ids::IdentityInterner;
use crate::compiler::input::{PackageKey, SourceContent, WorkspaceKey};

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
fn file_ranges_keep_stable_file_identity_separate_from_bytes() {
    let mut interner = IdentityInterner::default();
    let workspace = interner
        .workspace(&WorkspaceKey::ad_hoc("coordinates").unwrap())
        .unwrap();
    let package = interner
        .package(workspace, &PackageKey::command_line())
        .unwrap();
    let file = interner.file(package, "main.go").unwrap();
    let range = TextRange::new(TextSize::new(3), TextSize::new(8)).unwrap();
    let file_range = FileRange::new(file, range);

    assert_eq!(file_range.file(), file);
    assert_eq!(file_range.range(), range);
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

#[test]
fn physical_coordinates_use_utf8_byte_columns_and_include_eof() {
    let content = SourceContent::from_source("αβ\nz").unwrap();
    assert_eq!(content.text_len(), TextSize::new(6));
    assert_eq!(content.text_range(), TextRange::up_to(TextSize::new(6)));

    let middle_of_beta = content
        .physical_line_column(TextSize::new(3))
        .unwrap()
        .unwrap();
    assert_eq!(middle_of_beta.line().get(), 1);
    assert_eq!(middle_of_beta.byte_column().get(), 4);

    let second_line = content
        .physical_line_column(TextSize::new(5))
        .unwrap()
        .unwrap();
    assert_eq!(second_line.line().get(), 2);
    assert_eq!(second_line.byte_column().get(), 1);

    let eof = content
        .physical_line_column(TextSize::new(6))
        .unwrap()
        .unwrap();
    assert_eq!(eof.line().get(), 2);
    assert_eq!(eof.byte_column().get(), 2);
    assert_eq!(
        content.physical_line_column(TextSize::new(7)).unwrap(),
        None
    );

    let trailing_newline = SourceContent::from_source("x\n").unwrap();
    let eof_after_newline = trailing_newline
        .physical_line_column(TextSize::new(2))
        .unwrap()
        .unwrap();
    assert_eq!(eof_after_newline.line().get(), 2);
    assert_eq!(eof_after_newline.byte_column().get(), 1);
}
