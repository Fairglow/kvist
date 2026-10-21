use std::io::{self, Write};

use crate::theme::{ColorDepth, RenderOptions};

/// Encodes renderer-owned SGR for the host's capabilities without buffering
/// the document. Authored text must cross the inert-text boundary first.
pub(crate) struct Output<'a, W> {
    writer: &'a mut W,
    options: &'a RenderOptions,
    sequence: Vec<u8>,
}

impl<'a, W: Write> Output<'a, W> {
    pub fn new(writer: &'a mut W, options: &'a RenderOptions) -> Self {
        Self {
            writer,
            options,
            sequence: Vec::with_capacity(48),
        }
    }

    fn sgr(&mut self) -> io::Result<()> {
        if self.options.plain {
            return Ok(());
        }
        let source = std::str::from_utf8(&self.sequence[2..self.sequence.len() - 1])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let params: Vec<u16> = if source.is_empty() {
            vec![0]
        } else {
            source
                .split(';')
                .map(|p| {
                    p.parse::<u16>()
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
                })
                .collect::<io::Result<_>>()?
        };
        let mut index = 0;
        while index < params.len() {
            let parameter = params[index];
            if matches!(parameter, 38 | 48) {
                let background = parameter == 48;
                let (rgb, consumed) = match params.get(index + 1) {
                    Some(2) if index + 4 < params.len() => {
                        let values = &params[index + 2..index + 5];
                        if values.iter().any(|v| *v > 255) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid RGB color",
                            ));
                        }
                        ((values[0] as u8, values[1] as u8, values[2] as u8), 5)
                    }
                    Some(5) if index + 2 < params.len() && params[index + 2] <= 255 => {
                        let color = params[index + 2] as u8;
                        if self.options.color_depth == ColorDepth::Ansi256 {
                            write!(self.writer, "\x1b[{parameter};5;{color}m")?;
                            index += 3;
                            continue;
                        }
                        (indexed_rgb(color), 3)
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid extended color",
                        ));
                    }
                };
                match self.options.color_depth {
                    ColorDepth::None => {}
                    ColorDepth::TrueColor if consumed == 5 => {
                        write!(
                            self.writer,
                            "\x1b[{parameter};2;{};{};{}m",
                            rgb.0, rgb.1, rgb.2
                        )?;
                    }
                    ColorDepth::TrueColor => {
                        write!(self.writer, "\x1b[{parameter};5;{}m", params[index + 2])?;
                    }
                    ColorDepth::Ansi256 => {
                        write!(self.writer, "\x1b[{parameter};5;{}m", nearest(rgb, 256))?;
                    }
                    ColorDepth::Ansi16 => {
                        super::theme::Color::Ansi(nearest(rgb, 16))
                            .write(self.writer, background)?;
                    }
                }
                index += consumed;
            } else {
                let color = matches!(parameter, 30..=49 | 90..=107);
                if !color || self.options.color_depth != ColorDepth::None {
                    write!(self.writer, "\x1b[{parameter}m")?;
                }
                index += 1;
            }
        }
        Ok(())
    }
}

impl<W: Write> Write for Output<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if self.sequence.is_empty() && byte != 0x1b {
                continue;
            }
            if self.sequence.is_empty() {
                self.writer.write_all(&bytes[start..index])?;
            }
            self.sequence.push(byte);
            start = index + 1;
            if self.sequence.len() > 64
                || (self.sequence.len() == 2 && byte != b'[')
                || (self.sequence.len() > 2
                    && !byte.is_ascii_digit()
                    && !matches!(byte, b';' | b'm'))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected renderer control",
                ));
            }
            if byte == b'm' {
                self.sgr()?;
                self.sequence.clear();
            }
        }
        self.writer.write_all(&bytes[start..])?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.sequence.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete renderer control",
            ));
        }
        self.writer.flush()
    }
}

fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match index {
        0..=15 => BASE[usize::from(index)],
        16..=231 => {
            let index = index - 16;
            let channel = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            (
                channel(index / 36),
                channel(index / 6 % 6),
                channel(index % 6),
            )
        }
        _ => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

fn nearest(rgb: (u8, u8, u8), count: u16) -> u8 {
    (0..count)
        .min_by_key(|index| {
            let candidate = indexed_rgb(*index as u8);
            let distance = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
            distance(rgb.0, candidate.0)
                + distance(rgb.1, candidate.1)
                + distance(rgb.2, candidate.2)
        })
        .unwrap_or(0) as u8
}
