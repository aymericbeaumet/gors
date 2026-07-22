use crate::token::Token;
use phf::{Map, phf_map};
use unicode_general_category::{GeneralCategory, get_general_category};

// https://golang.org/ref/spec#Letters_and_digits
pub(super) fn is_letter(character: char) -> bool {
    character == '_' || is_unicode_letter(character)
}

pub(super) const fn is_octal_digit(character: char) -> bool {
    matches!(character, '0'..='7')
}

pub(super) const fn is_hex_digit(character: char) -> bool {
    character.is_ascii_hexdigit()
}

// https://golang.org/ref/spec#Characters
pub(super) const fn is_newline(character: char) -> bool {
    character == '\n'
}

fn is_unicode_letter(character: char) -> bool {
    matches!(
        get_general_category(character),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

pub(super) fn is_unicode_digit(character: char) -> bool {
    get_general_category(character) == GeneralCategory::DecimalNumber
}

// https://golang.org/ref/spec#Keywords
pub(super) static KEYWORDS: Map<&'static str, Token> = phf_map! {
  "break" => Token::BREAK,
  "case" => Token::CASE,
  "chan" => Token::CHAN,
  "const" => Token::CONST,
  "continue" => Token::CONTINUE,

  "default" => Token::DEFAULT,
  "defer" => Token::DEFER,
  "else" => Token::ELSE,
  "fallthrough" => Token::FALLTHROUGH,
  "for" => Token::FOR,

  "func" => Token::FUNC,
  "go" => Token::GO,
  "goto" => Token::GOTO,
  "if" => Token::IF,
  "import" => Token::IMPORT,

  "interface" => Token::INTERFACE,
  "map" => Token::MAP,
  "package" => Token::PACKAGE,
  "range" => Token::RANGE,
  "return" => Token::RETURN,

  "select" => Token::SELECT,
  "struct" => Token::STRUCT,
  "switch" => Token::SWITCH,
  "type" => Token::TYPE,
  "var" => Token::VAR,
};
