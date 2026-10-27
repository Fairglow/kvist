use blad::{Color, ColorDepth, MarkdownRenderer, Palette, RenderOptions, Theme};
use vte::{Params, Parser, Perform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Appearance {
    color: Option<Color>,
    bold: bool,
    italic: bool,
    strike: bool,
    reverse: bool,
}

#[derive(Default)]
struct StyledText {
    plain: String,
    text: String,
    styles: Vec<Appearance>,
    current: Appearance,
}

impl StyledText {
    fn parse(output: &str, independent_rows: bool) -> Self {
        let mut text = Self::default();
        let mut parser = Parser::new();
        for row in output.split_inclusive('\n') {
            if independent_rows {
                text.current = Appearance::default();
                parser = Parser::new();
            }
            parser.advance(&mut text, row.as_bytes());
        }
        text
    }

    fn assert_style(&self, text: &str, expected: Appearance) {
        let offset = self.text.find(text).unwrap_or_else(|| {
            panic!("missing {text:?} in {:?}", self.text);
        });
        let start = self.text[..offset].chars().count();
        let styles = &self.styles[start..start + text.chars().count()];
        assert!(
            styles.iter().all(|style| *style == expected),
            "{text:?}: expected {expected:?}, got {styles:?}"
        );
    }
}

impl Perform for StyledText {
    fn print(&mut self, character: char) {
        self.plain.push(character);
        if !character.is_whitespace() {
            self.text.push(character);
            self.styles.push(self.current);
        }
    }

    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.plain.push('\n');
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        if action != 'm' {
            return;
        }
        let mut values = params.iter().map(|value| value[0]);
        while let Some(value) = values.next() {
            match value {
                0 => self.current = Appearance::default(),
                1 => self.current.bold = true,
                3 => self.current.italic = true,
                9 => self.current.strike = true,
                7 => self.current.reverse = true,
                22 => self.current.bold = false,
                23 => self.current.italic = false,
                29 => self.current.strike = false,
                27 => self.current.reverse = false,
                30..=37 => self.current.color = Some(Color::Ansi((value - 30) as u8)),
                90..=97 => self.current.color = Some(Color::Ansi((value - 90 + 8) as u8)),
                39 => self.current.color = None,
                38 | 48 => {
                    let color = match values.next() {
                        Some(5) => Color::Ansi(values.next().unwrap().try_into().unwrap()),
                        Some(2) => Color::Rgb(
                            values.next().unwrap().try_into().unwrap(),
                            values.next().unwrap().try_into().unwrap(),
                            values.next().unwrap().try_into().unwrap(),
                        ),
                        other => panic!("unexpected color encoding: {other:?}"),
                    };
                    if value == 38 {
                        self.current.color = Some(color);
                    }
                }
                _ => {}
            }
        }
    }
}

fn render(source: &str, width: Option<usize>, options: RenderOptions) -> String {
    let mut renderer = MarkdownRenderer::with_options(options);
    renderer.set_width(width);
    let mut output = Vec::new();
    renderer.render_markdown(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn footnote_prose_retains_theme_across_wraps_breaks_and_paragraphs() {
    let source = "intro[^1]\n\n[^1]: first prose with a lengthy explanation\n    soft continuation  \n    hard continuation\n\n    second paragraph e\u{301}界👩‍💻unbrokencontinuationword\n\nordinary after\n\n[^name]: next definition\n\nordinary end";
    let muted = Appearance {
        color: Some(Color::Ansi(8)),
        ..Appearance::default()
    };
    for width in [Some(1), Some(12), Some(25), Some(80), None] {
        let output = render(source, width, RenderOptions::default());
        for independent_rows in [false, true] {
            let text = StyledText::parse(&output, independent_rows);
            text.assert_style("¹firstprosewithalengthyexplanation", muted);
            text.assert_style("softcontinuationhardcontinuation", muted);
            text.assert_style("secondparagraphe\u{301}界👩‍💻unbrokencontinuationword", muted);
            text.assert_style("[name]nextdefinition", muted);
            text.assert_style("ordinaryafter", Appearance::default());
            text.assert_style("ordinaryend", Appearance::default());
        }
        let plain = render(source, width, RenderOptions::plain());
        assert!(!plain.contains('\x1b'));
        assert_eq!(StyledText::parse(&output, false).plain, plain);
    }
}

#[test]
fn footnote_inline_styles_compose_and_restore_the_prose_theme() {
    let source = "intro[^1]\n\n[^1]: before **bold *both* boldend** afterbold *italic* afteritalic ~~strike~~ afterstrike `code` aftercode [**linked** label](https://example.com) afterlink [reference][ref] afterreference <https://auto.test> afterauto https://bare.test afterbare $x^2$ aftermath H~2~O afterscript [adjacent](https://example.com),tail\n\nordinary\n\n[ref]: https://reference.test";
    for width in [Some(8), Some(28), Some(80), None] {
        let output = render(source, width, RenderOptions::default());
        for independent_rows in [false, true] {
            let text = StyledText::parse(&output, independent_rows);
            let muted = Appearance {
                color: Some(Color::Ansi(8)),
                ..Appearance::default()
            };
            for word in [
                "before",
                "afterbold",
                "afteritalic",
                "afterstrike",
                "aftercode",
                "afterlink",
                "afterreference",
                "afterauto",
                "afterbare",
                "aftermath",
                "H₂O",
                "afterscript",
                ",tail",
            ] {
                text.assert_style(word, muted);
            }
            text.assert_style(
                "bold",
                Appearance {
                    bold: true,
                    ..muted
                },
            );
            text.assert_style(
                "both",
                Appearance {
                    bold: true,
                    italic: true,
                    ..muted
                },
            );
            text.assert_style(
                "boldend",
                Appearance {
                    bold: true,
                    ..muted
                },
            );
            text.assert_style(
                "italic",
                Appearance {
                    italic: true,
                    ..muted
                },
            );
            text.assert_style(
                "strike",
                Appearance {
                    strike: true,
                    ..muted
                },
            );
            text.assert_style(
                "code",
                Appearance {
                    reverse: true,
                    ..muted
                },
            );
            let link = Appearance {
                color: Some(Color::Ansi(14)),
                ..Appearance::default()
            };
            text.assert_style("linked", Appearance { bold: true, ..link });
            for word in [
                "label",
                "reference",
                "https://auto.test",
                "https://bare.test",
                "adjacent",
            ] {
                text.assert_style(word, link);
            }
            text.assert_style(
                "x²",
                Appearance {
                    color: Some(Color::Ansi(13)),
                    ..Appearance::default()
                },
            );
            text.assert_style("ordinary", Appearance::default());
        }
    }
}

#[test]
fn footnote_theme_obeys_palette_and_color_policy_without_geometry_changes() {
    let source =
        "intro[^1]\n\n[^1]: muted [link](https://example.com) restored **bold** tail\n\nordinary";
    for theme in [Theme::Dark, Theme::Light, Theme::Terminal] {
        for color_depth in [
            ColorDepth::None,
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            let options = RenderOptions {
                theme,
                color_depth,
                ..RenderOptions::default()
            };
            let output = render(source, Some(15), options.clone());
            let text = StyledText::parse(&output, true);
            let colored = color_depth != ColorDepth::None;
            let muted = Appearance {
                color: colored.then_some(Color::Ansi(8)),
                ..Appearance::default()
            };
            for word in ["¹muted", "restored", "tail"] {
                text.assert_style(word, muted);
            }
            text.assert_style(
                "link",
                Appearance {
                    color: colored.then_some(Palette::for_theme(theme).link),
                    ..Appearance::default()
                },
            );
            text.assert_style(
                "bold",
                Appearance {
                    bold: true,
                    ..muted
                },
            );
            let plain = render(
                source,
                Some(15),
                RenderOptions {
                    plain: true,
                    ..options
                },
            );
            assert!(!plain.contains('\x1b'));
            assert_eq!(text.plain, plain);
        }
    }
    let palette = Palette {
        muted: Color::Rgb(11, 22, 33),
        link: Color::Rgb(44, 55, 66),
        ..Palette::default()
    };
    let output = render(
        source,
        Some(12),
        RenderOptions {
            palette: Some(palette),
            ..RenderOptions::default()
        },
    );
    let text = StyledText::parse(&output, true);
    let muted = Appearance {
        color: Some(Color::Rgb(11, 22, 33)),
        ..Appearance::default()
    };
    text.assert_style("¹muted", muted);
    text.assert_style("restored", muted);
    text.assert_style(
        "bold",
        Appearance {
            bold: true,
            ..muted
        },
    );
    text.assert_style("tail", muted);
    text.assert_style(
        "link",
        Appearance {
            color: Some(Color::Rgb(44, 55, 66)),
            ..Appearance::default()
        },
    );
}
