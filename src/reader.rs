use enum_dispatch::enum_dispatch;

use crate::{Charset, LineEnding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewLineChar {
    Cr,
    Lf,
}

impl PartialEq<LineEnding> for NewLineChar {
    fn eq(&self, other: &LineEnding) -> bool {
        matches!(
            (self, other),
            (NewLineChar::Cr, LineEnding::Cr) | (NewLineChar::Lf, LineEnding::Lf)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentChar {
    Space,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharByteArray {
    pub len: u8,
    pub buffer: [u8; 4],
}

impl From<&[u8]> for CharByteArray {
    fn from(buf: &[u8]) -> Self {
        let mut buffer = std::mem::MaybeUninit::<[u8; 4]>::zeroed();
        unsafe {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), buffer.as_mut_ptr() as *mut u8, buf.len())
        };
        CharByteArray {
            len: buf.len() as u8,
            buffer: unsafe { buffer.assume_init() },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Character {
    Bom,
    Invalid(CharByteArray),
    NewLine(NewLineChar),
    Indent(IndentChar),
    Valid(CharByteArray),
}

#[enum_dispatch]
pub trait Reader {
    fn next(&mut self) -> std::io::Result<Option<Character>>;
}

pub struct Utf8Reader<T: std::io::Read + Sized>(T);
impl<T: std::io::Read + Sized> Reader for Utf8Reader<T> {
    fn next(&mut self) -> std::io::Result<Option<Character>> {
        let mut buf: [u8; 4] = [0; 4];
        let len = self.0.read(&mut buf[0..1])?;
        if len == 0 {
            Ok(None)
        } else {
            match buf[0] {
                b'\r' => Ok(Some(Character::NewLine(NewLineChar::Cr))),
                b'\n' => Ok(Some(Character::NewLine(NewLineChar::Lf))),
                b' ' => Ok(Some(Character::Indent(IndentChar::Space))),
                b'\t' => Ok(Some(Character::Indent(IndentChar::Tab))),
                ch if ch < 0x80 => Ok(Some(Character::Valid(buf[0..1].into()))),
                ch => {
                    // read more chars
                    let n = if ch & 0xF8 == 0xF0 {
                        4
                    } else if ch & 0xF0 == 0xE0 {
                        3
                    } else if ch & 0xE0 == 0xC0 {
                        2
                    } else {
                        return Ok(Some(Character::Invalid(buf[0..1].into())));
                    };

                    let len = self.0.read(&mut buf[1..n])?;
                    if len == n {
                        if let Ok(ch) = std::str::from_utf8(&buf[0..n]) {
                            return if ch == "\u{FEFF}" {
                                Ok(Some(Character::Bom))
                            } else {
                                Ok(Some(Character::Valid(buf[0..n].into())))
                            };
                        }
                    }

                    Ok(Some(Character::Invalid(buf[0..len].into())))
                }
            }
        }
    }
}

pub struct Latin1Reader<T: std::io::Read + Sized>(T);
impl<T: std::io::Read + Sized> Reader for Latin1Reader<T> {
    fn next(&mut self) -> std::io::Result<Option<Character>> {
        let mut buf: [u8; 1] = [0; 1];
        let len = self.0.read(&mut buf[..])?;
        if len == 0 {
            Ok(None)
        } else {
            match buf[0] {
                b'\r' => Ok(Some(Character::NewLine(NewLineChar::Cr))),
                b'\n' => Ok(Some(Character::NewLine(NewLineChar::Lf))),
                b' ' => Ok(Some(Character::Indent(IndentChar::Space))),
                b'\t' => Ok(Some(Character::Indent(IndentChar::Tab))),
                ch if !(0x7F..=0xA0).contains(&ch) => Ok(Some(Character::Valid(buf[0..1].into()))),
                _ => Ok(Some(Character::Invalid(buf[0..1].into()))),
            }
        }
    }
}

pub struct Utf16LeReader<T: std::io::Read + Sized>(T);
impl<T: std::io::Read + Sized> Reader for Utf16LeReader<T> {
    fn next(&mut self) -> std::io::Result<Option<Character>> {
        let mut buf: [u8; 4] = [0; 4];
        let len = self.0.read(&mut buf[0..2])?;
        match len {
            0 => Ok(None),
            1 => Ok(Some(Character::Invalid(buf[0..1].into()))),
            2 => match buf[0..2] {
                [b'\r', 0x00_u8] => Ok(Some(Character::NewLine(NewLineChar::Cr))),
                [b'\n', 0x00_u8] => Ok(Some(Character::NewLine(NewLineChar::Lf))),
                [b' ', 0x00_u8] => Ok(Some(Character::Indent(IndentChar::Space))),
                [b'\t', 0x00_u8] => Ok(Some(Character::Indent(IndentChar::Tab))),
                [0xFF_u8, 0xFE_u8] => Ok(Some(Character::Bom)),
                [_, ch] if (0xD8..=0xDB).contains(&ch) => {
                    let len = self.0.read(&mut buf[2..4])?;
                    if len == 2 && (0xDC..=0xDF).contains(&buf[3]) {
                        Ok(Some(Character::Valid(buf[0..4].into())))
                    } else {
                        Ok(Some(Character::Invalid(buf[0..(2 + len)].into())))
                    }
                }
                _ => Ok(Some(Character::Valid(buf[0..1].into()))),
            },
            _ => unreachable!("Not available"),
        }
    }
}
pub struct Utf16BeReader<T: std::io::Read + Sized>(T);
impl<T: std::io::Read + Sized> Reader for Utf16BeReader<T> {
    fn next(&mut self) -> std::io::Result<Option<Character>> {
        let mut buf: [u8; 4] = [0; 4];
        let len = self.0.read(&mut buf[0..2])?;
        match len {
            0 => Ok(None),
            1 => Ok(Some(Character::Invalid(buf[0..1].into()))),
            2 => match buf[0..2] {
                [0x00_u8, b'\r'] => Ok(Some(Character::NewLine(NewLineChar::Cr))),
                [0x00_u8, b'\n'] => Ok(Some(Character::NewLine(NewLineChar::Lf))),
                [0x00_u8, b' '] => Ok(Some(Character::Indent(IndentChar::Space))),
                [0x00_u8, b'\t'] => Ok(Some(Character::Indent(IndentChar::Tab))),
                [0xFE_u8, 0xFF_u8] => Ok(Some(Character::Bom)),
                [ch, _] if (0xD8..=0xDB).contains(&ch) => {
                    let len = self.0.read(&mut buf[2..4])?;
                    if len == 2 && (0xDC..=0xDF).contains(&buf[3]) {
                        Ok(Some(Character::Valid(buf[0..4].into())))
                    } else {
                        Ok(Some(Character::Invalid(buf[0..(2 + len)].into())))
                    }
                }
                _ => Ok(Some(Character::Valid(buf[0..1].into()))),
            },
            _ => unreachable!("Not available"),
        }
    }
}

pub struct UncheckedEncodingReader<T: std::io::Read + Sized>(T);

impl<T: std::io::Read + Sized> Reader for UncheckedEncodingReader<T> {
    fn next(&mut self) -> std::io::Result<Option<Character>> {
        let mut buf: [u8; 1] = [0; 1];
        let len = self.0.read(&mut buf[..])?;
        if len == 0 {
            Ok(None)
        } else {
            match buf[0] {
                b'\r' => Ok(Some(Character::NewLine(NewLineChar::Cr))),
                b'\n' => Ok(Some(Character::NewLine(NewLineChar::Lf))),
                b' ' => Ok(Some(Character::Indent(IndentChar::Space))),
                b'\t' => Ok(Some(Character::Indent(IndentChar::Tab))),
                _ => Ok(Some(Character::Valid(buf[0..1].into()))),
            }
        }
    }
}

#[enum_dispatch(Reader)]
pub enum CharacterReader<T: std::io::Read + Sized> {
    Utf8(Utf8Reader<T>),
    Latin1(Latin1Reader<T>),
    Utf16Le(Utf16LeReader<T>),
    Utf16Be(Utf16BeReader<T>),
    UncheckedEncoding(UncheckedEncodingReader<T>),
}

impl<T: std::io::Read + Sized> CharacterReader<T> {
    pub fn new(reader: T, charset: Option<Charset>) -> Self {
        match charset {
            Some(Charset::Latin1) => todo!(),
            Some(Charset::Utf8) | Some(Charset::Utf8WithBom) => {
                CharacterReader::Utf8(Utf8Reader(reader))
            }
            Some(Charset::Utf16BigEndian) => CharacterReader::Utf16Be(Utf16BeReader(reader)),
            Some(Charset::Utf16LittleEndian) => CharacterReader::Utf16Le(Utf16LeReader(reader)),
            None => CharacterReader::UncheckedEncoding(UncheckedEncodingReader(reader)),
        }
    }
}
