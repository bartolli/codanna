//! Table formatting utilities for structured output.

use comfy_table::{Attribute, Cell, Color, Table, presets::UTF8_FULL};

/// Builder for creating formatted tables.
pub struct TableBuilder {
    table: Table,
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TableBuilder {
    /// Create a new table builder.
    pub fn new() -> Self {
        let mut table = Table::new();
        table.load_style(UTF8_FULL.with_rounded_corners());
        Self { table }
    }

    /// Set the table headers.
    pub fn set_headers(mut self, headers: Vec<&str>) -> Self {
        let header_cells: Vec<Cell> = headers
            .into_iter()
            .map(|h| Cell::new(h).add_attribute(Attribute::Bold))
            .collect();
        self.table.set_header(header_cells);
        self
    }

    /// Add a row to the table.
    pub fn add_row(mut self, row: Vec<String>) -> Self {
        self.table.add_row(row);
        self
    }

    /// Build and return the formatted table.
    pub fn build(self) -> String {
        self.table.to_string()
    }
}

/// Create a benchmark results table.
pub fn create_benchmark_table(
    language: &str,
    file_path: Option<&str>,
    symbols: usize,
    avg_time: std::time::Duration,
    rate: f64,
) -> String {
    let mut table = Table::new();
    table.load_style(UTF8_FULL.with_rounded_corners());

    // Create the header
    table.set_header(vec![
        Cell::new("Metric").add_attribute(Attribute::Bold),
        Cell::new("Value").add_attribute(Attribute::Bold),
    ]);

    // Add rows without ANSI colors (comfy-table doesn't handle them well)
    table.add_row(vec!["Language", language]);

    if let Some(path) = file_path {
        table.add_row(vec!["File", path]);
    } else {
        table.add_row(vec!["File", "<generated benchmark code>"]);
    }

    table.add_row(vec!["Symbols parsed", &symbols.to_string()]);

    table.add_row(vec!["Average time", &format!("{avg_time:?}")]);

    table.add_row(vec!["Rate", &format!("{rate:.0} symbols/second")]);

    // Performance indicator with color
    let performance_ratio = rate / 10_000.0;
    let (performance_text, color) = if performance_ratio >= 1.0 {
        (
            format!("✓ {performance_ratio:.1}x faster than target"),
            Color::Green,
        )
    } else {
        (
            format!("⚠ {performance_ratio:.1}x of target"),
            Color::Yellow,
        )
    };

    table.add_row(vec![
        Cell::new("Performance"),
        Cell::new(performance_text)
            .fg(color)
            .add_attribute(Attribute::Bold),
    ]);

    table.to_string()
}

/// Create a summary table for indexing results.
pub fn create_summary_table(
    results: Vec<(String, usize, usize, std::time::Duration)>, // (language, files, symbols, time)
) -> String {
    let mut table = Table::new();
    table.load_style(UTF8_FULL.with_rounded_corners());

    // Header
    table.set_header(vec![
        Cell::new("Language").add_attribute(Attribute::Bold),
        Cell::new("Files").add_attribute(Attribute::Bold),
        Cell::new("Symbols").add_attribute(Attribute::Bold),
        Cell::new("Time").add_attribute(Attribute::Bold),
        Cell::new("Rate").add_attribute(Attribute::Bold),
    ]);

    // Data rows
    let mut total_files = 0;
    let mut total_symbols = 0;
    let mut total_time = std::time::Duration::ZERO;

    for (lang, files, symbols, time) in results {
        total_files += files;
        total_symbols += symbols;
        total_time += time;

        let rate = if time.as_secs_f64() > 0.0 {
            symbols as f64 / time.as_secs_f64()
        } else {
            0.0
        };

        table.add_row(vec![
            lang,
            files.to_string(),
            symbols.to_string(),
            format!("{:?}", time),
            format!("{:.0}/s", rate),
        ]);
    }

    // Total row
    if total_time.as_secs_f64() > 0.0 {
        let total_rate = total_symbols as f64 / total_time.as_secs_f64();
        table.add_row(vec![
            Cell::new("TOTAL").add_attribute(Attribute::Bold),
            Cell::new(total_files).add_attribute(Attribute::Bold),
            Cell::new(total_symbols).add_attribute(Attribute::Bold),
            Cell::new(format!("{total_time:?}")).add_attribute(Attribute::Bold),
            Cell::new(format!("{total_rate:.0}/s")).add_attribute(Attribute::Bold),
        ]);
    }

    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_builder() {
        let table = TableBuilder::new()
            .set_headers(vec!["Column 1", "Column 2"])
            .add_row(vec!["Value 1".to_string(), "Value 2".to_string()])
            .build();

        assert!(table.contains("Column 1"));
        assert!(table.contains("Value 1"));
    }
}
