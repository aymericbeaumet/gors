use super::lexical::{KEYWORDS, is_letter, is_newline, is_unicode_digit};
use super::{LineFilename, LineInfo, Result, Scanner, ScannerErrorKind, Step};
use crate::token::Token;

impl<'a> Scanner<'a> {
    // https://golang.org/ref/spec#Keywords
    // https://golang.org/ref/spec#Identifiers
    pub(super) fn scan_pkg_or_keyword_or_ident(&mut self) -> Result<Step<'a>> {
        self.next();

        while let Some(character) = self.current_char {
            if !(is_letter(character) || is_unicode_digit(character)) {
                break;
            }
            self.next();
        }

        let position = self.position();
        let literal = self.literal();

        if literal.len() > 1 {
            if let Some(&token) = KEYWORDS.get(literal) {
                self.insert_semi = matches!(
                    token,
                    Token::BREAK | Token::CONTINUE | Token::FALLTHROUGH | Token::RETURN
                );
                return Ok((position, token, literal));
            }
        }

        self.insert_semi = true;
        Ok((position, Token::IDENT, literal))
    }

    // https://golang.org/ref/spec#Integer_literals
    // https://golang.org/ref/spec#Floating-point_literals
    // https://golang.org/ref/spec#Imaginary_literals
    pub(super) fn scan_int_or_float_or_imag(&mut self, preceding_dot: bool) -> Result<Step<'a>> {
        self.insert_semi = true;

        let mut token = Token::INT;
        let mut digits = "_0123456789";
        let mut exp = "eE";

        if !preceding_dot {
            if matches!(self.current_char, Some('0')) {
                self.next();
                match self.current_char {
                    Some('b' | 'B') => {
                        digits = "_01";
                        exp = "";
                        self.next();
                    }
                    Some('o' | 'O') => {
                        digits = "_01234567";
                        exp = "";
                        self.next();
                    }
                    Some('x' | 'X') => {
                        digits = "_0123456789abcdefABCDEF";
                        exp = "pP";
                        self.next();
                    }
                    _ => {}
                };
            }

            while let Some(c) = self.current_char {
                if !digits.contains(c) {
                    break;
                }
                self.next();
            }
        }

        if preceding_dot || matches!(self.current_char, Some('.')) {
            token = Token::FLOAT;
            self.next();
            while let Some(c) = self.current_char {
                if !digits.contains(c) {
                    break;
                }
                self.next();
            }
        }

        if !exp.is_empty() {
            if let Some(c) = self.current_char {
                if exp.contains(c) {
                    token = Token::FLOAT;
                    self.next();
                    if matches!(self.current_char, Some('-' | '+')) {
                        self.next();
                    }
                    while let Some(c) = self.current_char {
                        if !matches!(c, '_' | '0'..='9') {
                            break;
                        }
                        self.next();
                    }
                }
            }
        }

        if matches!(self.current_char, Some('i')) {
            token = Token::IMAG;
            self.next();
        }

        Ok((self.position(), token, self.literal()))
    }

    // https://golang.org/ref/spec#Rune_literals
    pub(super) fn scan_rune(&mut self) -> Result<Step<'a>> {
        self.insert_semi = true;
        self.next();

        match self.current_char {
            Some('\\') => self.require_escaped_char::<'\''>()?,
            Some(_) => self.next(),
            _ => return Err(self.error(ScannerErrorKind::UnterminatedRune)),
        }

        if matches!(self.current_char, Some('\'')) {
            self.next();
            return Ok((self.position(), Token::CHAR, self.literal()));
        }

        Err(self.error(ScannerErrorKind::UnterminatedRune))
    }

    // https://golang.org/ref/spec#String_literals
    pub(super) fn scan_interpreted_string(&mut self) -> Result<Step<'a>> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.current_char {
            match c {
                '"' => {
                    self.next();
                    return Ok((self.position(), Token::STRING, self.literal()));
                }
                '\\' => self.require_escaped_char::<'"'>()?,
                _ => self.next(),
            }
        }

        Err(self.error(ScannerErrorKind::UnterminatedString))
    }

    // https://golang.org/ref/spec#String_literals
    pub(super) fn scan_raw_string(&mut self) -> Result<Step<'a>> {
        self.insert_semi = true;
        self.next();

        while let Some(c) = self.current_char {
            match c {
                '`' => {
                    self.next();
                    return Ok((self.position(), Token::STRING, self.literal()));
                }
                _ => self.next(),
            }
        }

        Err(self.error(ScannerErrorKind::UnterminatedString))
    }

    // https://golang.org/ref/spec#Comments
    pub(super) fn scan_general_comment(&mut self, track_semi_pos: bool) -> Result<Step<'a>> {
        self.next();
        self.next();

        let mut first_newline_pos: Option<(usize, usize, usize)> = None;

        while let Some(c) = self.current_char {
            // Track position of first newline for semicolon insertion
            if track_semi_pos && c == '\n' && first_newline_pos.is_none() {
                first_newline_pos = Some((self.offset, self.line, self.column));
            }

            match c {
                '*' => {
                    self.next();
                    if matches!(self.current_char, Some('/')) {
                        self.next();

                        // If the comment contained newlines, schedule a semicolon to be returned next
                        if let Some(pos) = first_newline_pos {
                            self.pending_semi = true;
                            self.pending_semi_pos = Some(pos);
                        } else if track_semi_pos {
                            // Comment doesn't contain newlines but find_line_end() was true,
                            // meaning there's a newline after the comment. Preserve insert_semi.
                            self.insert_semi = true;
                        }

                        let pos = self.position();
                        let lit = self.literal();

                        // look for compiler directives
                        self.directive(&lit["/*".len()..lit.len() - "*/".len()], true)?;

                        return Ok((pos, Token::COMMENT, lit));
                    }
                }
                _ => self.next(),
            }
        }

        Err(self.error(ScannerErrorKind::UnterminatedComment))
    }

    // https://golang.org/ref/spec#Comments
    pub(super) fn scan_line_comment(&mut self) -> Result<Step<'a>> {
        self.next();
        self.next();

        while let Some(c) = self.current_char {
            if is_newline(c) {
                break;
            }
            self.next();
        }

        let pos = self.position();
        let lit = self.literal();

        // Strip trailing \r from line comments (CRLF line endings)
        // Go's scanner does this to normalize line endings
        let lit = lit.strip_suffix('\r').unwrap_or(lit);

        // look for compiler directives (at the beginning of line)
        if self.start_physical_column == 1 {
            self.directive(&lit["//".len()..], false)?;
        }

        Ok((pos, Token::COMMENT, lit))
    }

    // https://pkg.go.dev/cmd/compile#hdr-Compiler_Directives
    fn directive(&mut self, input: &'a str, immediate: bool) -> Result<()> {
        if let Some(line_directive) = input.strip_prefix("line ") {
            self.pending_line_info = self.parse_line_directive(line_directive)?;
            if immediate {
                self.consume_pending_line_info();
            }
        }
        Ok(())
    }

    fn parse_line_directive(&mut self, line_directive: &'a str) -> Result<Option<LineInfo<'a>>> {
        const MAX_LINE_COLUMN: usize = 1 << 30;

        let Some((file, trailing)) = line_directive.rsplit_once(':') else {
            return Ok(None);
        };
        let trailing = parse_decimal(trailing)
            .filter(|value| (1..=MAX_LINE_COLUMN).contains(value))
            .ok_or_else(|| self.error(ScannerErrorKind::InvalidDirective))?;

        if let Some((filename, candidate_line)) = file.rsplit_once(':')
            && let Some(line) = parse_decimal(candidate_line)
        {
            if !(1..=MAX_LINE_COLUMN).contains(&line) {
                return Err(self.error(ScannerErrorKind::InvalidDirective));
            }
            // `filename:line:column`: an omitted filename retains the active
            // filename, unlike the two-field form which clears it.
            return Ok(Some(LineInfo {
                filename: if filename.is_empty() {
                    LineFilename::Retain
                } else {
                    LineFilename::Set(filename)
                },
                line,
                column: Some(trailing),
            }));
        }

        // `filename:line`: column information is hidden until the next
        // directive, and an omitted filename is the empty filename.
        Ok(Some(LineInfo {
            filename: if file.is_empty() {
                LineFilename::Clear
            } else {
                LineFilename::Set(file)
            },
            line: trailing,
            column: None,
        }))
    }

    pub(super) fn find_line_end(&self) -> bool {
        let buffer = self.buffer.as_bytes();
        let mut in_comment = true;

        let mut i = self.offset;
        while let Some(byte) = buffer.get(i) {
            let c = *byte as char;

            if let Some(next) = buffer.get(i + 1) {
                let n = *next as char;

                if !in_comment && c == '/' && n == '/' {
                    return true;
                }

                if c == '/' && n == '*' {
                    i += 2;
                    in_comment = true;
                    continue;
                }

                if c == '*' && n == '/' {
                    i += 2;
                    in_comment = false;
                    continue;
                }
            }

            if is_newline(c) {
                return true;
            }

            if !in_comment && !matches!(c, ' ' | '\t' | '\r') {
                return false;
            }

            i += 1;
        }

        !in_comment
    }
}

fn parse_decimal(input: &str) -> Option<usize> {
    (!input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| input.parse().ok())
        .flatten()
}
