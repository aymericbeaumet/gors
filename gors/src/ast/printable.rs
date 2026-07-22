mod declarations;
mod expressions;
mod statements;
mod support;

use std::io::Write;

/// Print a string using Go-compatible escape format.
/// Go's %q format preserves printable unicode characters and only escapes
/// control characters and non-printable characters.
pub(super) fn print_go_string<W: Write>(w: &mut W, s: &str) -> std::io::Result<()> {
    write!(w, "\"")?;
    for c in s.chars() {
        match c {
            '"' => write!(w, "\\\"")?,
            '\\' => write!(w, "\\\\")?,
            '\n' => write!(w, "\\n")?,
            '\r' => write!(w, "\\r")?,
            '\t' => write!(w, "\\t")?,
            // Control characters, surrogate pairs, and non-printable characters
            // Use Go-style escaping: \uXXXX for BMP, \UXXXXXXXX for supplementary
            c if !is_go_printable(c) => {
                let code = c as u32;
                if code <= 0xFFFF {
                    write!(w, "\\u{:04x}", code)?;
                } else {
                    write!(w, "\\U{:08x}", code)?;
                }
            }
            // All other characters (printable unicode) are kept as-is
            c => write!(w, "{}", c)?,
        }
    }
    write!(w, "\"")?;
    Ok(())
}

/// Check if a character is printable according to Go's strconv.IsPrint.
/// This matches Go's behavior for the %q format.
/// Go's IsPrint uses unicode.IsPrint which returns true only for:
/// - Letters (L category)
/// - Marks (M category)
/// - Numbers (N category)
/// - Punctuation (P category)
/// - Symbols (S category)
/// - ASCII space (U+0020)
fn is_go_printable(c: char) -> bool {
    use unicode_general_category::GeneralCategory::*;

    // ASCII space is explicitly printable
    if c == ' ' {
        return true;
    }

    // Use whitelist approach: only specific categories are printable
    match unicode_general_category::get_general_category(c) {
        // Letters (L)
        UppercaseLetter | LowercaseLetter | TitlecaseLetter | ModifierLetter | OtherLetter => true,
        // Marks (M)
        NonspacingMark | SpacingMark | EnclosingMark => true,
        // Numbers (N)
        DecimalNumber | LetterNumber | OtherNumber => true,
        // Punctuation (P)
        ConnectorPunctuation | DashPunctuation | OpenPunctuation | ClosePunctuation
        | InitialPunctuation | FinalPunctuation | OtherPunctuation => true,
        // Symbols (S)
        MathSymbol | CurrencySymbol | ModifierSymbol | OtherSymbol => true,
        // Everything else is not printable (including Unassigned, Control, Private Use, etc.)
        _ => false,
    }
}
