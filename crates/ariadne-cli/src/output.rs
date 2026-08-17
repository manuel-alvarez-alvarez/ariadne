//! Output helpers: aligned tables for humans, raw JSON with `--format json`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Table,
    Json,
}

/// Print any serializable value as pretty JSON.
pub fn print_json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Print rows as an aligned table with an uppercase header.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
    let line = |cells: Vec<String>| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                out.push_str("   ");
            }
            if i + 1 == cells.len() {
                out.push_str(cell);
            } else {
                out.push_str(&format!("{cell:<width$}", width = widths[i]));
            }
        }
        out
    };
    println!(
        "{}",
        line(headers.iter().map(|h| h.to_uppercase()).collect())
    );
    for row in rows {
        println!("{}", line(row.clone()));
    }
}

/// Print a key/value inspect block.
pub fn print_kv(pairs: &[(&str, String)]) {
    let width = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, v) in pairs {
        println!("{k:<width$}  {v}");
    }
}
