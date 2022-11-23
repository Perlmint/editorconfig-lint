use crate::{
    reader::{Character, CharacterReader, IndentChar, NewLineChar, Reader},
    Charset, Config, IndentStyle, LineEnding,
};

#[derive(Debug)]
pub enum Reason {
    IndentStyle,
    IndentSizeMismatch(usize),
    EndOfLineMismatch,
    TrailingWhiteSpaces,
    NoFinalNewline,
    BomNotFound,
    InvalidCharacter,
}

#[derive(Debug)]
pub struct Diagnosis {
    pub line: usize,
    pub range: (usize, usize),
    pub reason: Reason,
}

impl Diagnosis {
    pub fn fmt<O: std::io::Write, D: std::fmt::Display>(
        &self,
        out: &mut O,
        file_name: &D,
    ) -> std::io::Result<()> {
        write!(
            out,
            "error: {:?} at {}:{}:",
            self.reason, file_name, self.line
        )?;
        if self.range.0 == self.range.1 {
            writeln!(out, "{}", self.range.0)
        } else {
            writeln!(out, "{},{}", self.range.0, self.range.1)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Indent { len: usize, style_error: bool },
    NonWhitespace,
    NonIndentWhitespace { len: usize },
}

struct CheckState<'a> {
    line: usize,
    col: usize,
    state: State,
    prev_newline: Option<NewLineChar>,
    diagnosis: Vec<Diagnosis>,
    config: &'a Config,
}

impl<'a> CheckState<'a> {
    fn push_diagnosis(&mut self, diag: Diagnosis) {
        self.diagnosis.push(diag);
    }

    fn move_next_line(&mut self) {
        self.line += 1;
        self.col = 1;
    }

    fn check_end_of_newline(&mut self) {
        if let Some(new_line) = self.prev_newline.take() {
            if self
                .config
                .end_of_line
                .map(|v| new_line != v)
                .unwrap_or(false)
            {
                self.push_diagnosis(Diagnosis {
                    line: self.line,
                    range: (self.col - 1, self.col),
                    reason: Reason::EndOfLineMismatch,
                })
            }
            self.move_next_line();
        }
    }

    fn check_end_of_indent(&mut self, len: usize, style_error: bool) {
        if style_error {
            self.push_diagnosis(Diagnosis {
                line: self.line,
                range: (self.col - len, self.col),
                reason: Reason::IndentStyle,
            });
        } else if let Some(size) = self.config.indent_size {
            if len % size != 0 {
                self.push_diagnosis(Diagnosis {
                    line: self.line,
                    range: (self.col - len, self.col),
                    reason: Reason::IndentSizeMismatch(len),
                });
            }
        }
    }

    fn check_ch(&mut self, ch: Character) {
        match ch {
            Character::Indent(indent) => {
                match self.state {
                    State::NonWhitespace => self.state = State::NonIndentWhitespace { len: 1 },
                    State::Indent { len, style_error } => {
                        if len == 0 {
                            self.check_end_of_newline();
                        }
                        self.state = match (indent, self.config.indent_style) {
                            (IndentChar::Space, Some(IndentStyle::Tab)) => State::Indent {
                                len: len + 1,
                                style_error: true,
                            },
                            (IndentChar::Tab, Some(IndentStyle::Space)) => State::Indent {
                                len: len + 1,
                                style_error: true,
                            },
                            (_, _) => State::Indent {
                                len: len + 1,
                                style_error,
                            },
                        }
                    }
                    _ => {}
                }
                self.col += 1;
                self.prev_newline = None;
            }
            Character::NewLine(newline) => {
                let trailing = match self.state {
                    State::NonIndentWhitespace { len } => Some(len),
                    State::Indent { len, style_error } => {
                        if style_error {
                            self.push_diagnosis(Diagnosis {
                                line: self.line,
                                range: (self.col - len, self.col),
                                reason: Reason::IndentStyle,
                            });
                        }
                        Some(len)
                    }
                    _ => None,
                };
                if let Some(len) = trailing {
                    if len != 0 && self.config.trim_trailing_whitespace.unwrap_or(false) {
                        self.push_diagnosis(Diagnosis {
                            line: self.line,
                            range: (self.col - len, self.col),
                            reason: Reason::TrailingWhiteSpaces,
                        });
                    }
                }
                self.state = State::Indent {
                    len: 0,
                    style_error: false,
                };

                match (newline, self.prev_newline) {
                    (_, None) => {
                        self.prev_newline = Some(newline);
                    }
                    (NewLineChar::Lf, Some(NewLineChar::Lf)) => {
                        if self
                            .config
                            .end_of_line
                            .map(|v| v != LineEnding::Lf)
                            .unwrap_or(false)
                        {
                            self.push_diagnosis(Diagnosis {
                                line: self.line,
                                range: (self.col - 1, self.col),
                                reason: Reason::EndOfLineMismatch,
                            })
                        }
                        self.prev_newline = Some(NewLineChar::Lf);
                        self.move_next_line();
                    }
                    (NewLineChar::Lf, Some(NewLineChar::Cr)) => {
                        if self
                            .config
                            .end_of_line
                            .map(|v| v != LineEnding::Crlf)
                            .unwrap_or(false)
                        {
                            self.push_diagnosis(Diagnosis {
                                line: self.line,
                                range: (self.col - 1, self.col),
                                reason: Reason::EndOfLineMismatch,
                            })
                        }
                        self.prev_newline = None;
                        self.move_next_line();
                    }
                    (NewLineChar::Cr, Some(NewLineChar::Lf)) => {
                        self.push_diagnosis(Diagnosis {
                            line: self.line,
                            range: (self.col - 1, self.col),
                            reason: Reason::EndOfLineMismatch,
                        });
                        self.prev_newline = Some(NewLineChar::Cr);
                        self.move_next_line();
                    }
                    (NewLineChar::Cr, Some(NewLineChar::Cr)) => {
                        if self
                            .config
                            .end_of_line
                            .map(|v| v != LineEnding::Cr)
                            .unwrap_or(false)
                        {
                            self.push_diagnosis(Diagnosis {
                                line: self.line,
                                range: (self.col - 1, self.col),
                                reason: Reason::EndOfLineMismatch,
                            });
                        }
                        self.prev_newline = Some(NewLineChar::Cr);
                        self.move_next_line();
                    }
                }
            }
            Character::Valid(_) => {
                match self.state {
                    State::Indent { len, style_error } => {
                        self.check_end_of_newline();
                        self.check_end_of_indent(len, style_error);
                    }
                    State::NonWhitespace | State::NonIndentWhitespace { .. } => {}
                }
                self.state = State::NonWhitespace;
                self.col += 1;
            }
            Character::Invalid(_) | Character::Bom => {
                match self.state {
                    State::Indent { len, style_error } => {
                        self.check_end_of_newline();
                        self.check_end_of_indent(len, style_error);
                    }
                    State::NonWhitespace | State::NonIndentWhitespace { .. } => {}
                }
                self.push_diagnosis(Diagnosis {
                    line: self.line,
                    range: (self.col - 1, self.col),
                    reason: Reason::InvalidCharacter,
                });
                self.state = State::NonWhitespace;
                self.col += 1;
            }
        }
    }
}

pub fn check<R: std::io::BufRead>(input: R, config: Config) -> std::io::Result<Vec<Diagnosis>> {
    let mut state = CheckState {
        line: 1,
        col: 1,
        state: State::Indent {
            len: 0,
            style_error: false,
        },
        prev_newline: None,
        diagnosis: Vec::new(),
        config: &config,
    };

    let mut reader = CharacterReader::new(input, config.charset);

    match config.charset {
        Some(Charset::Latin1) | Some(Charset::Utf8) | None => {
            // no bom check
        }
        Some(Charset::Utf8WithBom) => {
            let ch = reader.next()?;
            if ch != Some(Character::Bom) {
                state.push_diagnosis(Diagnosis {
                    line: 1,
                    range: (0, 0),
                    reason: Reason::BomNotFound,
                });

                if let Some(ch) = ch {
                    state.check_ch(ch);
                }
            }
        }
        Some(Charset::Utf16BigEndian) | Some(Charset::Utf16LittleEndian) => {
            let ch = reader.next()?;
            if ch != Some(Character::Bom) {
                if let Some(ch) = ch {
                    state.check_ch(ch);
                }
            }
        }
    }

    while let Some(ch) = reader.next()? {
        state.check_ch(ch)
    }

    Ok(state.diagnosis)
}
