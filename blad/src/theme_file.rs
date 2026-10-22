use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;

use blad::{Color, Palette, Theme};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ThemeFile {
    pub theme: Option<Theme>,
    palette: BTreeMap<String, serde_json::Value>,
}

impl ThemeFile {
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "theme file exceeds 64 KiB",
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn palette(&self, mut palette: Palette) -> io::Result<Palette> {
        for (key, value) in &self.palette {
            let invalid = || {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid theme palette field: {key}"),
                )
            };
            match key.as_str() {
                "heading" => palette.heading = color(value).ok_or_else(invalid)?,
                "link" => palette.link = color(value).ok_or_else(invalid)?,
                "muted" => palette.muted = color(value).ok_or_else(invalid)?,
                "list_marker" => palette.list_marker = color(value).ok_or_else(invalid)?,
                "math" => palette.math = color(value).ok_or_else(invalid)?,
                "chrome" => palette.chrome = color(value).ok_or_else(invalid)?,
                "search" => palette.search = color(value).ok_or_else(invalid)?,
                "alerts" => {
                    let values = value
                        .as_array()
                        .filter(|values| values.len() == 5)
                        .ok_or_else(invalid)?;
                    for (slot, value) in palette.alerts.iter_mut().zip(values) {
                        *slot = color(value).ok_or_else(invalid)?;
                    }
                }
                "code_background" | "table_background" | "quote_background" => {
                    let fill = if value.is_null() {
                        None
                    } else {
                        Some(color(value).ok_or_else(invalid)?)
                    };
                    match key.as_str() {
                        "code_background" => palette.code_background = fill,
                        "table_background" => palette.table_background = fill,
                        _ => palette.quote_background = fill,
                    }
                }
                _ => return Err(invalid()),
            }
        }
        Ok(palette)
    }
}

fn color(value: &serde_json::Value) -> Option<Color> {
    if let Some(index) = value.as_u64().and_then(|index| u8::try_from(index).ok()) {
        return Some(Color::Ansi(index));
    }
    let hex = value.as_str()?.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&hex[..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..], 16).ok()?,
    ))
}
