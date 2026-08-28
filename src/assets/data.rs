//! External data for charts: a CSV or a JSON read off disk, the same way a
//! still or a clip is.
//!
//! A chart's numbers do not have to live inline in the film. A
//! [`crate::chart::Series::Data`] names a file and the columns to plot, and
//! this module turns that file into named columns of values. It is resolved
//! through [`crate::assets::AssetStore`] like every other asset, decoded once
//! and cached, so twenty worker threads plotting the same table read one copy.
//!
//! The one rule that shapes it: **a table that will not become the chart it is
//! asked for is an error at load, not an empty plot at render.** A missing
//! file, a missing column, a cell that is not a number — each fails loudly and
//! names the file and the row, because a chart that quietly plots nothing is
//! the exact bug this feature exists to make impossible.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// One parsed data file: named columns, each a list of cells in row order.
///
/// Both supported formats collapse to the same shape — a set of named columns
/// of equal length — so the chart code that reads it never learns which format
/// it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct DataTable {
    /// For error messages: the file this came from.
    source: String,
    /// Column names, in the order they appeared.
    names: Vec<String>,
    /// One vector of cells per column, parallel to `names`; every column has
    /// the same length (the row count).
    columns: Vec<Vec<Cell>>,
}

/// One cell: a number, or text that could not be read as one.
#[derive(Debug, Clone, PartialEq)]
enum Cell {
    Num(f64),
    Text(String),
}

impl Cell {
    fn parse(raw: &str) -> Cell {
        // An empty cell is text (an absent value), not zero — plotting a gap as
        // a hard zero is the quiet-wrong-chart failure in miniature.
        match raw.trim().parse::<f64>() {
            Ok(n) if !raw.trim().is_empty() => Cell::Num(n),
            _ => Cell::Text(raw.to_string()),
        }
    }

    fn as_str(&self) -> String {
        match self {
            Cell::Num(n) => crate::chart::format_tick(*n),
            Cell::Text(s) => s.clone(),
        }
    }
}

impl DataTable {
    /// Load and parse a data file, choosing the parser by extension: `.csv`
    /// for comma-separated values with a header row, `.json` for JSON. Any
    /// other extension is an error rather than a guess.
    pub fn load(path: &Path) -> Result<DataTable> {
        let source = path.display().to_string();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading data file {source}"))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Self::from_csv(&source, &text),
            "json" => Self::from_json(&source, &text),
            other => bail!(
                "data file {source}: unknown extension {other:?}; ShowReel reads .csv and .json"
            ),
        }
    }

    /// Parse CSV text: the first non-empty line is the header naming the
    /// columns; every later line is a row. Quoting is deliberately minimal — a
    /// data export for a chart is numbers and short labels, not prose — but a
    /// field wrapped in double quotes may contain commas.
    pub fn from_csv(source: &str, text: &str) -> Result<DataTable> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let (_, header) = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("data file {source}: is empty"))?;
        let names: Vec<String> = split_csv(header).iter().map(|s| s.trim().to_string()).collect();
        if names.iter().any(|n| n.is_empty()) {
            bail!("data file {source}: a column in the header row has no name");
        }
        let mut columns: Vec<Vec<Cell>> = vec![Vec::new(); names.len()];
        for (i, line) in lines {
            let fields = split_csv(line);
            if fields.len() != names.len() {
                // `i` is the zero-based enumerate index; report the 1-based file
                // line so it matches what an editor shows.
                bail!(
                    "data file {source}: row {} has {} fields but the header has {}",
                    i + 1,
                    fields.len(),
                    names.len()
                );
            }
            for (c, f) in fields.iter().enumerate() {
                columns[c].push(Cell::parse(f));
            }
        }
        Ok(DataTable { source: source.to_string(), names, columns })
    }

    /// Parse JSON in either of the two shapes a data export usually takes: an
    /// array of row objects (`[{"age":0,"users":10}, …]`) or an object of
    /// named columns (`{"age":[0,1], "users":[10,20]}`).
    pub fn from_json(source: &str, text: &str) -> Result<DataTable> {
        let value: serde_json::Value =
            serde_json::from_str(text).with_context(|| format!("data file {source}: not valid JSON"))?;
        match value {
            serde_json::Value::Array(rows) => Self::from_json_rows(source, rows),
            serde_json::Value::Object(map) => Self::from_json_columns(source, map),
            _ => bail!(
                "data file {source}: JSON data must be an array of row objects or an object of columns"
            ),
        }
    }

    fn from_json_rows(source: &str, rows: Vec<serde_json::Value>) -> Result<DataTable> {
        // Column order comes from the first row; later rows fill by name, so a
        // row missing a key is a loud error rather than a silent shift.
        let first = rows
            .first()
            .and_then(|r| r.as_object())
            .ok_or_else(|| anyhow::anyhow!("data file {source}: the first row is not an object"))?;
        let names: Vec<String> = first.keys().cloned().collect();
        let mut columns: Vec<Vec<Cell>> = vec![Vec::new(); names.len()];
        for (i, row) in rows.iter().enumerate() {
            let obj = row
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("data file {source}: row {i} is not an object"))?;
            for (c, name) in names.iter().enumerate() {
                let cell = obj.get(name).ok_or_else(|| {
                    anyhow::anyhow!("data file {source}: row {i} has no {name:?} field")
                })?;
                columns[c].push(json_cell(cell));
            }
        }
        Ok(DataTable { source: source.to_string(), names, columns })
    }

    fn from_json_columns(
        source: &str,
        map: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DataTable> {
        let mut names = Vec::new();
        let mut columns: Vec<Vec<Cell>> = Vec::new();
        let mut len: Option<usize> = None;
        for (name, val) in map {
            let arr = val.as_array().ok_or_else(|| {
                anyhow::anyhow!("data file {source}: column {name:?} is not an array")
            })?;
            if let Some(l) = len
                && l != arr.len()
            {
                bail!(
                    "data file {source}: column {name:?} has {} values but another has {l}",
                    arr.len()
                );
            }
            len = Some(arr.len());
            columns.push(arr.iter().map(json_cell).collect());
            names.push(name);
        }
        Ok(DataTable { source: source.to_string(), names, columns })
    }

    /// How many rows the table holds.
    pub fn rows(&self) -> usize {
        self.columns.first().map(Vec::len).unwrap_or(0)
    }

    fn index_of(&self, name: &str) -> Result<usize> {
        self.names.iter().position(|n| n == name).ok_or_else(|| {
            anyhow::anyhow!(
                "data file {}: no column named {name:?}; it has {}",
                self.source,
                self.names
                    .iter()
                    .map(|n| format!("{n:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }

    /// A column read as numbers. A cell that is not a number is an error that
    /// names the file, the row and the offending text.
    pub fn column_f64(&self, name: &str) -> Result<Vec<f64>> {
        let idx = self.index_of(name)?;
        self.columns[idx]
            .iter()
            .enumerate()
            .map(|(row, cell)| match cell {
                Cell::Num(n) => Ok(*n),
                Cell::Text(s) => bail!(
                    "data file {}: column {name:?} row {row} is {s:?}, which is not a number",
                    self.source
                ),
            })
            .collect()
    }

    /// A column read as text — for category labels, where numbers are fine too
    /// (they are formatted the same way a chart tick is).
    pub fn column_str(&self, name: &str) -> Result<Vec<String>> {
        let idx = self.index_of(name)?;
        Ok(self.columns[idx].iter().map(Cell::as_str).collect())
    }

    /// Approximate bytes held, for [`crate::assets::AssetStore::memory_bytes`].
    pub fn memory_bytes(&self) -> usize {
        self.columns
            .iter()
            .flat_map(|c| c.iter())
            .map(|cell| match cell {
                Cell::Num(_) => 8,
                Cell::Text(s) => s.len() + 16,
            })
            .sum()
    }
}

fn json_cell(v: &serde_json::Value) -> Cell {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(Cell::Num).unwrap_or(Cell::Text(n.to_string())),
        serde_json::Value::String(s) => Cell::parse(s),
        serde_json::Value::Bool(b) => Cell::Text(b.to_string()),
        serde_json::Value::Null => Cell::Text(String::new()),
        other => Cell::Text(other.to_string()),
    }
}

/// Split one CSV line into fields, honouring double-quoted fields (which may
/// contain commas and doubled `""` for a literal quote). Small on purpose: a
/// chart's data is numbers and labels, not a spreadsheet dump.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => out.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    out.push(field);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_reads_named_columns() {
        let t = DataTable::from_csv("t.csv", "age,users\n0,10\n40,80\n").unwrap();
        assert_eq!(t.rows(), 2);
        assert_eq!(t.column_f64("age").unwrap(), vec![0.0, 40.0]);
        assert_eq!(t.column_f64("users").unwrap(), vec![10.0, 80.0]);
    }

    #[test]
    fn a_missing_column_names_what_exists() {
        let t = DataTable::from_csv("t.csv", "age,users\n0,10\n").unwrap();
        let err = t.column_f64("year").unwrap_err().to_string();
        assert!(err.contains("year"), "{err}");
        assert!(err.contains("age") && err.contains("users"), "{err}");
    }

    #[test]
    fn a_non_numeric_cell_names_the_row() {
        let t = DataTable::from_csv("t.csv", "x,y\n0,10\n1,oops\n").unwrap();
        let err = t.column_f64("y").unwrap_err().to_string();
        assert!(err.contains("row 1"), "{err}");
        assert!(err.contains("oops"), "{err}");
    }

    #[test]
    fn a_ragged_row_fails_at_the_row() {
        let err = DataTable::from_csv("t.csv", "x,y\n0,10\n1\n").unwrap_err().to_string();
        assert!(err.contains("row 3"), "{err}");
    }

    #[test]
    fn json_array_of_rows() {
        let t = DataTable::from_json("t.json", r#"[{"x":0,"y":10},{"x":1,"y":20}]"#).unwrap();
        assert_eq!(t.column_f64("x").unwrap(), vec![0.0, 1.0]);
        assert_eq!(t.column_f64("y").unwrap(), vec![10.0, 20.0]);
    }

    #[test]
    fn json_object_of_columns() {
        let t = DataTable::from_json("t.json", r#"{"x":[0,1],"y":[10,20]}"#).unwrap();
        assert_eq!(t.column_f64("x").unwrap(), vec![0.0, 1.0]);
        assert_eq!(t.column_f64("y").unwrap(), vec![10.0, 20.0]);
    }

    #[test]
    fn a_missing_field_in_a_row_is_loud() {
        let err = DataTable::from_json("t.json", r#"[{"x":0,"y":10},{"x":1}]"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("row 1") && err.contains("\"y\""), "{err}");
    }

    #[test]
    fn category_labels_read_as_text() {
        let t = DataTable::from_csv("t.csv", "region,sales\nNorth,10\nSouth,20\n").unwrap();
        assert_eq!(t.column_str("region").unwrap(), vec!["North", "South"]);
        assert_eq!(t.column_f64("sales").unwrap(), vec![10.0, 20.0]);
    }

    #[test]
    fn quoted_fields_keep_commas() {
        let f = split_csv(r#"a,"b,c",d"#);
        assert_eq!(f, vec!["a", "b,c", "d"]);
    }
}
