#![allow(clippy::unwrap_used)]

use super::{CanonicalImportPath, ImportPathIssue};

#[test]
fn accepts_and_preserves_canonical_paths() {
    let path = CanonicalImportPath::new("example.com/team/module/v2").unwrap();

    assert_eq!(path.as_str(), "example.com/team/module/v2");
    assert_eq!(path.shared().as_ref(), "example.com/team/module/v2");
}

#[test]
fn rejects_paths_that_can_escape_or_alias_filesystem_names() {
    for (path, expected) in [
        ("", ImportPathIssue::Empty),
        ("/absolute", ImportPathIssue::Absolute),
        ("C:/absolute", ImportPathIssue::Absolute),
        ("example.com\\pkg", ImportPathIssue::Backslash),
        ("example.com//pkg", ImportPathIssue::EmptyElement),
        (
            "example.com/../pkg",
            ImportPathIssue::DotElement("..".to_owned()),
        ),
        (
            "example.com/CON",
            ImportPathIssue::ReservedWindowsName("CON".to_owned()),
        ),
        (
            "example.com/LONGNA~1",
            ImportPathIssue::WindowsShortName("LONGNA~1".to_owned()),
        ),
    ] {
        assert_eq!(CanonicalImportPath::new(path).unwrap_err(), expected);
    }
}
