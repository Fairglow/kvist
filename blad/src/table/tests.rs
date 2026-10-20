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

// This oracle enumerates actual widths, not height caps or solver bounds.
fn exhaustive(table: &Table, viewport: usize) {
    let natural = table.widths(None);
    let upper = natural.iter().sum::<usize>();
    let overhead = table.columns.len().saturating_sub(1) * 3;
    let mut best_fit = usize::MAX;
    let mut best_overflow = (usize::MAX, usize::MAX);
    let mut candidates = vec![vec![]];
    for _ in &table.columns {
        candidates = candidates
            .into_iter()
            .flat_map(|prefix| {
                (4..=upper).map(move |w| {
                    let mut widths = prefix.clone();
                    widths.push(w);
                    widths
                })
            })
            .collect();
    }
    for widths in candidates {
        let h = height(table, &widths);
        if widths.iter().any(|&w| w < 4 * h)
            || table.rows.iter().any(|row| {
                row.iter()
                    .zip(&widths)
                    .any(|(cell, &w)| cell.wrap(w).iter().any(|line| line.width() > w))
            })
        {
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
    assert!(actual.iter().all(|&w| w >= 4 * h), "{actual:?}");
    if best_fit != usize::MAX {
        assert!(width <= viewport, "{actual:?}, viewport {viewport}");
        assert_eq!(h, best_fit, "{actual:?}, viewport {viewport}");
    } else {
        assert_eq!((width, h), best_overflow, "{actual:?}, viewport {viewport}");
    }
}

#[test]
fn exact_solver_matches_exhaustive_small_tables() {
    let texts = ["", "a", "a b c", "ab cd ef gh", "abcdefgh", "a\nb\n"];
    for a in texts {
        for b in texts {
            let table = table(&[&["A", "B"], &[a, b]]);
            for viewport in [0, 8, 11, 15, 19, 25, 40] {
                exhaustive(&table, viewport);
            }
        }
    }
    let table = table(&[&["a b c d", "x"], &["y", "ab cd ef gh"]]);
    for viewport in 0..=30 {
        exhaustive(&table, viewport);
    }
    let three = Table::new(&[vec!["a b c".into(), "x y z".into(), "q".into()]]);
    for viewport in [0, 14, 19, 25, 40] {
        exhaustive(&three, viewport);
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
    assert_eq!(t.widths(Some(0)), [4, 4]);
    assert_eq!(t.rows[1][1].wrap(4), [""]);
    assert_eq!(table(&[&["x\nx\nx", ""]]).widths(None), [12, 12]);
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
    assert!(widths.iter().all(|&w| w >= 4 * height(&t, &widths)));
    assert_eq!(t.widths(Some(usize::MAX)), t.widths(None));
}
