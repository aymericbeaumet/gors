//! Top-level token dispatch and automatic semicolon insertion.

use super::lexical::is_letter;
use super::{Result, Scanner, ScannerErrorKind, Step};
use crate::token::{Position, Token};

impl<'a> Scanner<'a> {
    #[allow(clippy::cognitive_complexity)] // Token dispatch mirrors Go's lexical grammar.
    pub fn scan(&mut self) -> Result<Step<'a>> {
        // Check for pending semicolon (from multi-line comment with newlines)
        if self.pending_semi {
            self.pending_semi = false;
            let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take() {
                Position {
                    origin: self.origin,
                    offset,
                    line,
                    column: if self.hide_column { 0 } else { column },
                }
            } else {
                self.position()
            };
            return Ok((pos, Token::SEMICOLON, "\n"));
        }

        let insert_semi = self.insert_semi;
        self.insert_semi = false;

        while let Some(c) = self.current_char {
            self.reset_start();

            match c {
                ' ' | '\t' | '\r' => {
                    self.next();
                }

                '\n' => {
                    self.next();
                    if insert_semi {
                        let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take()
                        {
                            Position {
                                origin: self.origin,
                                offset,
                                line,
                                column: if self.hide_column { 0 } else { column },
                            }
                        } else {
                            self.position()
                        };
                        return Ok((pos, Token::SEMICOLON, "\n"));
                    }
                }

                _ => break,
            }
        }

        if let Some(c) = self.current_char {
            match c {
                '+' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::ADD_ASSIGN, ""));
                        }
                        Some('+') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.position(), Token::INC, ""));
                        }
                        _ => return Ok((self.position(), Token::ADD, "")),
                    }
                }

                '-' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::SUB_ASSIGN, ""));
                        }
                        Some('-') => {
                            self.insert_semi = true;
                            self.next();
                            return Ok((self.position(), Token::DEC, ""));
                        }
                        _ => return Ok((self.position(), Token::SUB, "")),
                    }
                }

                '*' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::MUL_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::MUL, "")),
                    }
                }

                '/' => match self.peek() {
                    Some('=') => {
                        self.next();
                        self.next();
                        return Ok((self.position(), Token::QUO_ASSIGN, ""));
                    }
                    Some('/') => {
                        // Line comments: scan the comment first, preserve semicolon insertion
                        // for the newline that follows. This matches Go's scanner behavior.
                        if insert_semi {
                            self.insert_semi = true;
                        }
                        return self.scan_line_comment();
                    }
                    Some('*') => {
                        // General comments: scan the comment first, preserve semicolon insertion
                        // if this comment extends to end of line. This matches Go's scanner behavior.
                        let track_semi_pos = insert_semi && self.find_line_end();
                        // Note: we don't set self.insert_semi here - scan_general_comment will
                        // set pending_semi if the comment contains newlines, which handles the
                        // semicolon insertion. If the comment doesn't contain newlines but
                        // find_line_end() returned true, we need to preserve insert_semi.
                        return self.scan_general_comment(track_semi_pos);
                    }
                    _ => {
                        self.next();
                        return Ok((self.position(), Token::QUO, ""));
                    }
                },

                '%' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::REM_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::REM, "")),
                    }
                }

                '&' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::AND_ASSIGN, ""));
                        }
                        Some('&') => {
                            self.next();
                            return Ok((self.position(), Token::LAND, ""));
                        }
                        Some('^') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::AND_NOT_ASSIGN, ""));
                                }
                                _ => return Ok((self.position(), Token::AND_NOT, "")),
                            }
                        }
                        _ => return Ok((self.position(), Token::AND, "")),
                    }
                }

                '|' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::OR_ASSIGN, ""));
                        }
                        Some('|') => {
                            self.next();
                            return Ok((self.position(), Token::LOR, ""));
                        }
                        _ => return Ok((self.position(), Token::OR, "")),
                    }
                }

                '^' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::XOR_ASSIGN, ""));
                        }
                        _ => return Ok((self.position(), Token::XOR, "")),
                    }
                }

                '<' => {
                    self.next();
                    match self.current_char {
                        Some('<') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::SHL_ASSIGN, ""));
                                }
                                _ => return Ok((self.position(), Token::SHL, "")),
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::LEQ, ""));
                        }
                        Some('-') => {
                            self.next();
                            return Ok((self.position(), Token::ARROW, ""));
                        }
                        _ => return Ok((self.position(), Token::LSS, "")),
                    }
                }

                '>' => {
                    self.next();
                    match self.current_char {
                        Some('>') => {
                            self.next();
                            match self.current_char {
                                Some('=') => {
                                    self.next();
                                    return Ok((self.position(), Token::SHR_ASSIGN, ""));
                                }
                                _ => {
                                    return Ok((self.position(), Token::SHR, ""));
                                }
                            }
                        }
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::GEQ, ""));
                        }
                        _ => return Ok((self.position(), Token::GTR, "")),
                    }
                }

                ':' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::DEFINE, ""));
                        }
                        _ => return Ok((self.position(), Token::COLON, "")),
                    }
                }

                '!' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::NEQ, ""));
                        }
                        _ => return Ok((self.position(), Token::NOT, "")),
                    }
                }

                ',' => {
                    self.next();
                    return Ok((self.position(), Token::COMMA, ""));
                }

                '(' => {
                    self.next();
                    return Ok((self.position(), Token::LPAREN, ""));
                }

                ')' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RPAREN, ""));
                }

                '[' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACK, ""));
                }

                ']' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACK, ""));
                }

                '{' => {
                    self.next();
                    return Ok((self.position(), Token::LBRACE, ""));
                }

                '}' => {
                    self.insert_semi = true;
                    self.next();
                    return Ok((self.position(), Token::RBRACE, ""));
                }

                '~' => {
                    self.next();
                    return Ok((self.position(), Token::TILDE, ""));
                }

                ';' => {
                    self.next();
                    return Ok((self.position(), Token::SEMICOLON, ";"));
                }

                '.' => {
                    self.next();
                    match self.current_char {
                        Some('0'..='9') => return self.scan_int_or_float_or_imag(true),
                        Some('.') => match self.peek() {
                            Some('.') => {
                                self.next();
                                self.next();
                                return Ok((self.position(), Token::ELLIPSIS, ""));
                            }
                            _ => return Ok((self.position(), Token::PERIOD, "")),
                        },
                        _ => return Ok((self.position(), Token::PERIOD, "")),
                    }
                }

                '=' => {
                    self.next();
                    match self.current_char {
                        Some('=') => {
                            self.next();
                            return Ok((self.position(), Token::EQL, ""));
                        }
                        _ => return Ok((self.position(), Token::ASSIGN, "")),
                    }
                }

                '0'..='9' => return self.scan_int_or_float_or_imag(false),
                '\'' => return self.scan_rune(),
                '"' => return self.scan_interpreted_string(),
                '`' => return self.scan_raw_string(),
                c if is_letter(c) => return self.scan_pkg_or_keyword_or_ident(),
                _ => {
                    return Err(self.error(ScannerErrorKind::IllegalCharacter));
                }
            };
        }

        self.reset_start();
        if insert_semi {
            let pos = if let Some((offset, line, column)) = self.pending_semi_pos.take() {
                Position {
                    origin: self.origin,
                    offset,
                    line,
                    column: if self.hide_column { 0 } else { column },
                }
            } else {
                self.position()
            };
            Ok((pos, Token::SEMICOLON, "\n"))
        } else {
            Ok((self.position(), Token::EOF, ""))
        }
    }
}
