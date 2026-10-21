use std::fmt;

use crate::MathMode;
use crate::scripts::{self, Position};

const MAX_INPUT: usize = 16_384;
const MAX_DEPTH: usize = 32;
const MAX_OUTPUT: usize = 65_536;

#[derive(Debug)]
pub(crate) enum Error {
    Unsupported(String),
    Invalid(&'static str),
    Resource(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(reason) => write!(f, "unsupported: {reason}"),
            Self::Invalid(reason) => write!(f, "invalid: {reason}"),
            Self::Resource(reason) => write!(f, "resource limit: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn present(tex: &str, original: &str, mode: MathMode, currency: bool) -> String {
    if mode == MathMode::Source || currency {
        return original.to_owned();
    }
    match project(tex) {
        Ok(text) => text,
        Err(error) => {
            let source = if matches!(error, Error::Resource(_)) && original.len() > MAX_INPUT {
                let mut end = MAX_INPUT;
                while !original.is_char_boundary(end) {
                    end -= 1;
                }
                format!(
                    "{} [... {} source bytes omitted]",
                    &original[..end],
                    original.len() - end
                )
            } else {
                original.to_owned()
            };
            format!("{source} [math {error}]")
        }
    }
}

pub(crate) fn currency(tex: &str, following: Option<char>) -> bool {
    let amount = tex.trim().trim_start_matches(['+', '-']);
    let numeric = !amount.is_empty()
        && amount.chars().any(|c| c.is_ascii_digit())
        && amount
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ','));
    numeric
        || (amount.starts_with(|c: char| c.is_ascii_digit())
            && following.is_some_and(|c| c.is_ascii_digit())
            && !tex.contains(['\\', '^', '_', '{', '}']))
}

fn project(tex: &str) -> Result<String, Error> {
    if tex.len() > MAX_INPUT {
        return Err(Error::Resource("formula exceeds 16384 input bytes"));
    }
    let mut parser = Parser {
        source: tex,
        cursor: 0,
    };
    let output = parser.expression(0, Stop::End)?;
    if output.trim().is_empty() {
        return Err(Error::Invalid("empty formula"));
    }
    Ok(output.trim().to_owned())
}

#[derive(Clone, Copy)]
enum Stop {
    End,
    Group,
    Right,
}

struct Parser<'a> {
    source: &'a str,
    cursor: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn take(&mut self) -> Option<char> {
        let next = self.peek()?;
        self.cursor += next.len_utf8();
        Some(next)
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.take();
        }
    }

    fn depth(depth: usize) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            Err(Error::Resource("formula nesting exceeds 32"))
        } else {
            Ok(())
        }
    }

    fn append(output: &mut String, text: &str) -> Result<(), Error> {
        if output.len().saturating_add(text.len()) > MAX_OUTPUT {
            return Err(Error::Resource("formula exceeds 65536 output bytes"));
        }
        output.push_str(text);
        Ok(())
    }

    fn wrapped(prefix: &str, body: &str, suffix: &str) -> Result<String, Error> {
        let mut output = String::new();
        for part in [prefix, body, suffix] {
            Self::append(&mut output, part)?;
        }
        Ok(output)
    }

    fn command_is(&self, command: &str) -> bool {
        self.source[self.cursor..]
            .strip_prefix('\\')
            .and_then(|tail| tail.strip_prefix(command))
            .is_some_and(|tail| !tail.starts_with(|c: char| c.is_ascii_alphabetic()))
    }

    fn expression(&mut self, depth: usize, stop: Stop) -> Result<String, Error> {
        Self::depth(depth)?;
        let mut output = String::new();
        loop {
            let Some(character) = self.peek() else {
                return match stop {
                    Stop::End => Ok(output),
                    Stop::Group => Err(Error::Invalid("missing closing brace")),
                    Stop::Right => Err(Error::Invalid("missing paired \\right")),
                };
            };
            if (matches!(stop, Stop::Group) && character == '}')
                || (matches!(stop, Stop::Right) && self.command_is("right"))
            {
                return Ok(output);
            }
            if character.is_whitespace() {
                self.whitespace();
                Self::append(&mut output, " ")?;
                continue;
            }
            let fraction = self.command_is("frac");
            let mut group_scripts = fraction || self.command_is("text");
            let mut atom = self.atom(depth)?;
            let (mut sub, mut sup) = (false, false);
            loop {
                let before_space = self.cursor;
                self.whitespace();
                let position = match self.peek() {
                    Some('_') if !sub => {
                        sub = true;
                        Position::Sub
                    }
                    Some('^') if !sup => {
                        sup = true;
                        Position::Super
                    }
                    Some('_' | '^') => return Err(Error::Invalid("duplicate script on one atom")),
                    _ => {
                        self.cursor = before_space;
                        break;
                    }
                };
                self.take();
                self.whitespace();
                if atom.trim().is_empty() {
                    return Err(Error::Invalid("script without a preceding atom"));
                }
                if group_scripts {
                    atom = Self::wrapped("(", &atom, ")")?;
                    group_scripts = false;
                }
                let argument = if self.peek() == Some('{') {
                    self.group(depth + 1)?
                } else {
                    self.atom(depth + 1)?
                };
                if argument.trim().is_empty() {
                    return Err(Error::Invalid("empty script argument"));
                }
                Self::append(&mut atom, &scripts::span(&argument, position))?;
            }
            if fraction && group_scripts {
                let position = self.cursor;
                self.whitespace();
                let at_end = self.peek().is_none()
                    || (matches!(stop, Stop::Group) && self.peek() == Some('}'))
                    || (matches!(stop, Stop::Right) && self.command_is("right"));
                self.cursor = position;
                if !output.trim().is_empty() || !at_end {
                    atom = Self::wrapped("(", &atom, ")")?;
                }
            }
            Self::append(&mut output, &atom)?;
        }
    }

    fn group(&mut self, depth: usize) -> Result<String, Error> {
        Self::depth(depth)?;
        self.whitespace();
        if self.take() != Some('{') {
            return Err(Error::Invalid("expected a braced argument"));
        }
        let body = self.expression(depth, Stop::Group)?;
        if self.take() != Some('}') {
            return Err(Error::Invalid("missing closing brace"));
        }
        if body.trim().is_empty() {
            return Err(Error::Invalid("empty grouped argument"));
        }
        Ok(body)
    }

    fn atom(&mut self, depth: usize) -> Result<String, Error> {
        Self::depth(depth)?;
        match self.peek() {
            None => Err(Error::Invalid("missing argument")),
            Some('{') => Self::wrapped("(", &self.group(depth + 1)?, ")"),
            Some('}') => Err(Error::Invalid("unexpected closing brace")),
            Some('_' | '^') => Err(Error::Invalid("script without a preceding atom")),
            Some('$' | '&' | '%' | '#') => Err(Error::Unsupported(
                "alignment, comments and macro parameters are not supported".into(),
            )),
            Some('\\') => {
                self.take();
                self.command(depth)
            }
            Some(character) => {
                self.take();
                Ok(character.to_string())
            }
        }
    }

    fn name(&mut self) -> &str {
        let start = self.cursor;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.take();
        }
        &self.source[start..self.cursor]
    }

    fn command(&mut self, depth: usize) -> Result<String, Error> {
        if !self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return match self.take() {
                Some(character @ ('{' | '}' | '$' | '%' | '_' | '#' | '&')) => {
                    Ok(character.to_string())
                }
                Some(',' | ';' | ':' | ' ') => Ok(" ".into()),
                Some('!') => Ok(String::new()),
                _ => Err(Error::Unsupported(
                    "unknown escape or line-break command".into(),
                )),
            };
        }
        let start = self.cursor;
        let name = self.name().to_owned();
        match name.as_str() {
            "frac" => {
                let numerator = self.group(depth + 1)?;
                let denominator = self.group(depth + 1)?;
                let mut result = Self::wrapped("(", &numerator, ")/(")?;
                Self::append(&mut result, &denominator)?;
                Self::append(&mut result, ")")?;
                Ok(result)
            }
            "sqrt" => {
                self.whitespace();
                if self.peek() == Some('[') {
                    return Err(Error::Unsupported(
                        "optional root indices are not supported".into(),
                    ));
                }
                Self::wrapped("√(", &self.group(depth + 1)?, ")")
            }
            "text" => self.literal_text(depth + 1),
            "left" => self.paired(depth + 1),
            "right" => Err(Error::Invalid("\\right has no matching \\left")),
            "quad" | "qquad" | "space" => Ok(" ".into()),
            _ => symbol(&name).map(str::to_owned).ok_or_else(|| {
                let command: String = self.source[start..self.cursor].chars().take(64).collect();
                Error::Unsupported(format!("unknown command \\{command}"))
            }),
        }
    }

    fn literal_text(&mut self, depth: usize) -> Result<String, Error> {
        Self::depth(depth)?;
        self.whitespace();
        if self.take() != Some('{') {
            return Err(Error::Invalid("\\text requires braces"));
        }
        let mut nesting = 1;
        let mut output = String::new();
        while let Some(character) = self.take() {
            match character {
                '{' => {
                    nesting += 1;
                    Self::depth(depth + nesting - 1)?;
                }
                '}' => {
                    nesting -= 1;
                    if nesting == 0 {
                        return Ok(output);
                    }
                }
                '\\' => match self.take() {
                    Some(escaped @ ('{' | '}' | '$' | '%' | '_' | '#' | '&')) => {
                        Self::append(&mut output, &escaped.to_string())?;
                        continue;
                    }
                    _ => {
                        return Err(Error::Unsupported(
                            "commands inside \\text are not supported".into(),
                        ));
                    }
                },
                _ => {}
            }
            Self::append(&mut output, &character.to_string())?;
        }
        Err(Error::Invalid("missing closing \\text brace"))
    }

    fn delimiter(&mut self) -> Result<&'static str, Error> {
        self.whitespace();
        match self.take() {
            Some('(') => Ok("("),
            Some(')') => Ok(")"),
            Some('[') => Ok("["),
            Some(']') => Ok("]"),
            Some('|') => Ok("|"),
            Some('\\') => match self.peek() {
                Some('{') => {
                    self.take();
                    Ok("{")
                }
                Some('}') => {
                    self.take();
                    Ok("}")
                }
                Some('|') => {
                    self.take();
                    Ok("‖")
                }
                _ => match self.name() {
                    "langle" => Ok("⟨"),
                    "rangle" => Ok("⟩"),
                    "lVert" | "rVert" | "Vert" => Ok("‖"),
                    _ => Err(Error::Unsupported("unsupported paired delimiter".into())),
                },
            },
            _ => Err(Error::Unsupported("unsupported paired delimiter".into())),
        }
    }

    fn paired(&mut self, depth: usize) -> Result<String, Error> {
        Self::depth(depth)?;
        let opening = self.delimiter()?;
        let expected = match opening {
            "(" => ")",
            "[" => "]",
            "{" => "}",
            "|" => "|",
            "⟨" => "⟩",
            "‖" => "‖",
            _ => return Err(Error::Invalid("invalid opening \\left delimiter")),
        };
        let body = self.expression(depth, Stop::Right)?;
        self.take();
        if self.name() != "right" {
            return Err(Error::Invalid("missing paired \\right"));
        }
        if self.delimiter()? != expected {
            return Err(Error::Invalid("mismatched paired delimiters"));
        }
        Self::wrapped(opening, &body, expected)
    }
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "omicron" => "ο",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "times" => "×",
        "cdot" => "·",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "propto" => "∝",
        "infty" => "∞",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "neg" => "¬",
        "land" => "∧",
        "lor" => "∨",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "mapsto" => "↦",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "oint" => "∮",
        "partial" => "∂",
        "nabla" => "∇",
        "degree" => "°",
        "ldots" => "…",
        "cdots" => "⋯",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "log" => "log",
        "ln" => "ln",
        "lim" => "lim",
        "exp" => "exp",
        "max" => "max",
        "min" => "min",
        "det" => "det",
        "gcd" => "gcd",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_budget_accepts_the_limit_and_rejects_the_next_byte() {
        let mut output = "x".repeat(MAX_OUTPUT);
        assert!(Parser::append(&mut output, "").is_ok());
        assert!(matches!(
            Parser::append(&mut output, "x"),
            Err(Error::Resource(_))
        ));
        assert_eq!(output.len(), MAX_OUTPUT);
    }
}
