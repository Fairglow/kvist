use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Parses a shell-free command template and substitutes its supported values.
pub fn render_command(
    template: &str,
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
) -> Result<(String, Vec<String>)> {
    let raw_arguments = parse_arguments(template)?;
    let Some(program) = raw_arguments.first().cloned() else {
        return Err(Error::InvalidCommandTemplate {
            reason: "template must contain a program".to_owned(),
        });
    };
    let mut arguments = Vec::new();

    for raw_argument in &raw_arguments[1..] {
        let mut rendered = raw_argument
            .replace("{prompt_json}", &json_string(prompt))
            .replace("{prompt}", prompt);
        if rendered.contains("{target_directory}") {
            let directory = target_dir
                .to_str()
                .ok_or_else(|| Error::InvalidCommandTemplate {
                    reason: format!(
                        "target directory `{}` is not valid UTF-8",
                        target_dir.display()
                    ),
                })?;
            rendered = rendered.replace("{target_directory}", directory);
        }

        if raw_argument.contains("{context_files}") {
            if context_paths.is_empty() {
                if raw_argument == "{context_files}"
                    && arguments
                        .last()
                        .is_some_and(|argument: &String| argument.starts_with('-'))
                {
                    arguments.pop();
                } else {
                    arguments.push(rendered.replace("{context_files}", ""));
                }
                continue;
            }
            for path in context_paths {
                let path = path.to_str().ok_or_else(|| Error::InvalidCommandTemplate {
                    reason: format!("context path `{}` is not valid UTF-8", path.display()),
                })?;
                arguments.push(rendered.replace("{context_files}", path));
            }
        } else {
            arguments.push(rendered);
        }
    }

    Ok((program, arguments))
}

pub(crate) fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{08}' => encoded.push_str("\\b"),
            '\u{0c}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character <= '\u{1f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                encoded.push_str("\\u00");
                let byte = character as usize;
                encoded.push(HEX[byte >> 4] as char);
                encoded.push(HEX[byte & 0x0f] as char);
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

/// Splits a quoted command into a program and arguments without interpolation.
pub fn split_raw_command(template: &str) -> Result<(String, Vec<String>)> {
    let mut arguments = parse_arguments(template)?;
    if arguments.is_empty() {
        return Err(Error::InvalidCommandTemplate {
            reason: "template must contain a program".to_owned(),
        });
    }
    let program = arguments.remove(0);
    Ok((program, arguments))
}

fn parse_arguments(template: &str) -> Result<Vec<String>> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    let mut characters = template.chars().peekable();

    while let Some(character) = characters.next() {
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (Some(active), character) if character == active => {
                quote = None;
            }
            (None, character) if character.is_whitespace() => {
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (Some(active), '\\') => match characters.peek().copied() {
                Some(next) if next == active || next == '\\' => {
                    current.push(next);
                    characters.next();
                    started = true;
                }
                _ => {
                    current.push('\\');
                    started = true;
                }
            },
            (_, character) => {
                current.push(character);
                started = true;
            }
        }
    }

    if let Some(quote) = quote {
        return Err(Error::InvalidCommandTemplate {
            reason: format!("unterminated `{quote}` quote"),
        });
    }
    if started {
        arguments.push(current);
    }

    Ok(arguments)
}
