//! Exact integer constraints for borderless, whole-word-wrapped tables.
//!
//! A display column is modeled as half a line high. A visual 2:1 cell
//! therefore targets four columns per occupied text line, capped at natural
//! text width. Empty height and neighboring cells never widen a column.

mod cell;

use cell::Cell;

const COLUMNS_PER_LINE: usize = 4;
const SEPARATOR_WIDTH: usize = 3;

#[derive(Default)]
struct Column {
    minimum: usize,
    natural: usize,
}

pub(crate) struct Table {
    pub rows: Vec<Vec<Cell>>,
    columns: Vec<Column>,
    explicit_height: usize,
}

impl Table {
    pub fn new(rows: &[Vec<String>]) -> Self {
        let count = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut columns: Vec<_> = (0..count).map(|_| Column::default()).collect();
        let mut explicit_height = 1;
        let rows = rows
            .iter()
            .map(|row| {
                columns
                    .iter_mut()
                    .enumerate()
                    .map(|(index, column)| {
                        let cell = Cell::new(row.get(index).map_or("", String::as_str));
                        column.minimum = column.minimum.max(cell.minimum);
                        column.natural = column.natural.max(cell.natural);
                        explicit_height = explicit_height.max(cell.explicit_height());
                        cell
                    })
                    .collect()
            })
            .collect();
        Self {
            rows,
            columns,
            explicit_height,
        }
    }

    /// Minimize maximum row height when fitting, otherwise total overflow width.
    ///
    /// At each height cap the columns are independent. Their smallest feasible
    /// widths give an exact feasibility test, not a continuous approximation.
    pub fn widths(&self, viewport: Option<usize>) -> Vec<usize> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        let natural: Vec<_> = self.columns.iter().map(|column| column.natural).collect();
        let Some(viewport) = viewport else {
            return natural;
        };
        let overhead = self
            .columns
            .len()
            .saturating_sub(1)
            .saturating_mul(SEPARATOR_WIDTH);
        let budget = viewport.saturating_sub(overhead);
        if total(&natural) <= budget {
            return natural;
        }

        let mut widths: Vec<_> = self.columns.iter().map(|column| column.minimum).collect();
        if total(&widths) >= budget {
            return widths;
        }

        let mut low = self.explicit_height;
        let mut high = self
            .rows
            .iter()
            .flat_map(|row| row.iter().zip(&widths).map(|(cell, &w)| cell.height(w)))
            .max()
            .unwrap_or(1);
        while low < high {
            let mid = low + (high - low) / 2;
            if self.fits_budget(mid, budget) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        for (column, width) in widths.iter_mut().enumerate() {
            *width = self.required_width(column, low);
        }
        let spare = budget - total(&widths);
        self.distribute(&mut widths, spare);
        widths
    }

    fn fits_budget(&self, cap: usize, budget: usize) -> bool {
        let mut used = 0usize;
        for column in 0..self.columns.len() {
            used = used.saturating_add(self.required_width(column, cap));
            if used > budget {
                return false;
            }
        }
        true
    }

    fn required_width(&self, column: usize, cap: usize) -> usize {
        let mut low = self.columns[column].minimum;
        let mut high = self.columns[column].natural;
        while low < high {
            let mid = low + (high - low) / 2;
            if self
                .rows
                .iter()
                .all(|row| row[column].fits_height(mid, cap))
            {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        low
    }

    fn distribute(&self, widths: &mut [usize], spare: usize) {
        let flexibility: Vec<_> = self
            .columns
            .iter()
            .zip(widths.iter())
            .map(|(column, &width)| column.natural.saturating_sub(width))
            .collect();
        let total_flex = total(&flexibility);
        if total_flex == 0 {
            return;
        }
        let spare = spare.min(total_flex);
        let mut remaining = spare;
        let mut remainders = Vec::with_capacity(widths.len());
        for (index, (width, flex)) in widths.iter_mut().zip(flexibility).enumerate() {
            let product = flex as u128 * spare as u128;
            let share = (product / total_flex as u128) as usize;
            *width += share;
            remaining -= share;
            remainders.push((product % total_flex as u128, index));
        }
        remainders.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, index) in remainders.into_iter().take(remaining) {
            widths[index] += 1;
        }
    }
}

fn total(widths: &[usize]) -> usize {
    widths
        .iter()
        .fold(0, |sum, &width| sum.saturating_add(width))
}

#[cfg(test)]
mod tests;
