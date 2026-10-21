use super::*;
use unicode_width::UnicodeWidthStr;

fn table(rows: &[&[&str]]) -> Table {
    Table::new(
        &rows
            .iter()
            .map(|row| row.iter().map(|s| (*s).to_owned()).collect())
            .collect::<Vec<_>>(),
    )
}

fn height(table: &Table, widths: &[usize]) -> usize {
    table
        .rows
        .iter()
        .flat_map(|row| row.iter().zip(widths).map(|(cell, &w)| cell.wrap(w).len()))
        .max()
        .unwrap_or(1)
}

fn natural(cell: &Cell) -> usize {
    cell.wrap(usize::MAX)
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or(0)
}

fn valid_text_width(cell: &Cell, width: usize) -> bool {
    let lines = cell.wrap(width);
    let occupied = lines.iter().filter(|line| !line.is_empty()).count();
    lines.iter().all(|line| line.width() <= width)
        && (width >= natural(cell) || width >= 4 * occupied)
}

// This oracle enumerates actual widths, not height caps or solver bounds.
fn exhaustive(table: &Table, viewport: usize) {
    let natural: Vec<_> = (0..table.columns.len())
        .map(|column| {
            table
                .rows
                .iter()
                .map(|row| natural(&row[column]))
                .max()
                .unwrap_or(0)
        })
        .collect();
    assert_eq!(table.widths(None), natural);
    let overhead = table.columns.len().saturating_sub(1) * 3;
    let mut best_fit = usize::MAX;
    let mut best_overflow = (usize::MAX, usize::MAX);
    let mut candidates = vec![vec![]];
    for &upper in &natural {
        candidates = candidates
            .into_iter()
            .flat_map(|prefix| {
                (0..=upper).map(move |w| {
                    let mut widths = prefix.clone();
                    widths.push(w);
                    widths
                })
            })
            .collect();
    }
    for widths in candidates {
        let h = height(table, &widths);
        if table.rows.iter().any(|row| {
            row.iter()
                .zip(&widths)
                .any(|(cell, &w)| !valid_text_width(cell, w))
        }) {
            continue;
        }
        let width = widths.iter().sum::<usize>() + overhead;
        if width <= viewport {
            best_fit = best_fit.min(h);
        }
        best_overflow = best_overflow.min((width, h));
    }
    let actual = table.widths(Some(viewport));
    let h = height(table, &actual);
    let width = actual.iter().sum::<usize>() + overhead;
    assert!(
        actual.iter().zip(&natural).all(|(w, upper)| w <= upper),
        "{actual:?}"
    );
    assert!(table.rows.iter().all(|row| {
        row.iter()
            .zip(&actual)
            .all(|(cell, &w)| valid_text_width(cell, w))
    }));
    if best_fit != usize::MAX {
        assert!(width <= viewport, "{actual:?}, viewport {viewport}");
        assert_eq!(h, best_fit, "{actual:?}, viewport {viewport}");
    } else {
        assert_eq!((width, h), best_overflow, "{actual:?}, viewport {viewport}");
    }
}

#[test]
fn exact_solver_matches_exhaustive_small_tables() {
    let texts = [
        "",
        "a",
        "a b c",
        "ab cd ef gh",
        "abcdefgh",
        "a\nb\n",
        "\n\n",
    ];
    for a in texts {
        for b in texts {
            let table = table(&[&["A", "B"], &[a, b]]);
            for viewport in [0, 8, 11, 15, 19, 25, 40] {
                exhaustive(&table, viewport);
            }
        }
    }
    let crossed = table(&[&["a b c d", "x"], &["y", "ab cd ef gh"]]);
    for viewport in 0..=30 {
        exhaustive(&crossed, viewport);
    }
    let three = Table::new(&[vec!["a b c".into(), "x y z".into(), "q".into()]]);
    for viewport in [0, 14, 19, 25, 40] {
        exhaustive(&three, viewport);
    }
    // A short multiline value is exempt at its own natural width, not the
    // column's larger natural width supplied by a different row.
    let mixed = table(&[&["a\nb\nc", "one two three"], &["xy", "x"]]);
    for viewport in 0..=25 {
        exhaustive(&mixed, viewport);
    }
    let paragraphs = table(&[&[
        "a b c d e f g h i j k l m n o p q r s t",
        "one two three four five six seven eight nine ten",
    ]]);
    for viewport in [0, 40, 45, 50, 60, 70, 80, 90] {
        exhaustive(&paragraphs, viewport);
    }
}

#[test]
fn cell_floor_matches_exhaustive_text_geometry() {
    let mut texts: Vec<String> = [
        "",
        "\n\n",
        "a\nb\nc",
        "世界 世界 世界",
        "e\u{301} 👩‍💻",
        "ab\n\ncd\n",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for mut shape in 0..243 {
        let mut words = Vec::new();
        for _ in 0..5 {
            words.push("x".repeat(shape % 3 + 1));
            shape /= 3;
        }
        texts.push(words.join(" "));
    }
    for text in texts {
        let cell = Cell::new(&text);
        let expected = (0..=natural(&cell))
            .find(|&width| valid_text_width(&cell, width))
            .unwrap();
        assert_eq!(cell.minimum, expected, "{text:?}");
        assert_eq!(
            Cell::new(&format!("\n\n{text}\n\n")).minimum,
            expected,
            "{text:?}"
        );
        assert!(
            (expected..=natural(&cell)).all(|width| valid_text_width(&cell, width)),
            "{text:?}"
        );
    }
}

#[test]
fn wrap_measurement_matches_output_and_is_monotone() {
    for text in [
        "",
        "one  two\tthree",
        "\n\n",
        "ab\n\ncd\n",
        "世界 世界",
        "e\u{301} 👩‍💻",
        "\x1b[2Jbad\x07",
    ] {
        let cell = Cell::new(text);
        let mut previous = usize::MAX;
        for width in cell.minimum.max(1)..=cell.natural.max(1) + 5 {
            let lines = cell.wrap(width);
            assert!(!lines.is_empty());
            assert!(lines.iter().all(|line| line.width() <= width));
            assert!(lines.len() <= previous);
            for cap in 0..=lines.len() + 1 {
                assert_eq!(cell.fits_height(width, cap), lines.len() <= cap);
            }
            previous = lines.len();
        }
    }
    assert_eq!(Cell::new("one  two\tthree").wrap(80), ["one two three"]);
    assert_eq!(Cell::new("a\n\nb\n").wrap(80), ["a", "", "b", ""]);
    assert_eq!(Cell::new("👩‍💻 e\u{301}").wrap(2), ["👩‍💻", "e\u{301}"]);
    assert_eq!(Cell::new("\x1b[2Jx").wrap(80), ["\\u{1b}[2Jx"]);
}

#[test]
fn empty_ragged_and_unbounded_tables() {
    assert!(table(&[]).widths(Some(0)).is_empty());
    assert!(table(&[&[]]).widths(None).is_empty());
    let t = table(&[&["x", "y"], &["z"]]);
    assert_eq!(t.widths(Some(0)), [1, 1]);
    assert_eq!(t.rows[1][1].wrap(4), [""]);
    assert_eq!(table(&[&["x\nx\nx", ""]]).widths(None), [1, 0]);
    assert_eq!(table(&[&["", ""]]).widths(Some(0)), [0, 0]);
    assert_eq!(table(&[&["", ""]]).widths(Some(100)), [0, 0]);
    assert_eq!(t.widths(Some(usize::MAX)), t.widths(None));
}

#[test]
fn sufficient_space_avoids_wrapping_and_rounding_is_stable() {
    let t = table(&[&["a b c d e f g", "a b c d e f g", "a b c d e f g"]]);
    assert_eq!(t.widths(Some(100)), [13, 13, 13]);
    assert_eq!(t.widths(Some(32)), [9, 9, 8]);
}

#[test]
fn large_input_and_large_viewport_do_not_expand_the_search_space() {
    let words = "word ".repeat(10_000);
    let t = Table::new(&[vec!["x".to_owned(), words]]);
    let widths = t.widths(Some(0));
    assert_eq!(widths[0], 1);
    assert!(
        t.rows[0]
            .iter()
            .zip(&widths)
            .all(|(cell, &w)| valid_text_width(cell, w))
    );
    assert_eq!(t.widths(Some(usize::MAX)), t.widths(None));
}

#[test]
fn column_floors_ignore_neighbors_and_empty_explicit_height() {
    let description = "one two three four five six seven eight nine ten";
    let plain = table(&[&["id", "p/", description]]);
    let padded = table(&[&["id", "p/", &format!("\n\n{description}\n\n")]]);
    assert_eq!(plain.widths(Some(0)), padded.widths(Some(0)));
    for viewport in [0, 1, 12, 30, 80, usize::MAX] {
        let widths = plain.widths(Some(viewport));
        assert_eq!(&widths[..2], [2, 2]);
        assert!(
            plain.rows[0]
                .iter()
                .zip(&widths)
                .all(|(cell, &w)| valid_text_width(cell, w))
        );
    }
}
