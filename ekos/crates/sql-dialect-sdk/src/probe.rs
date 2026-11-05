//! Running read-only SQL against a database for checks (RFC 0242/0243). A database plugin
//! implements [`Probe`] over its own read-only session; `ekos-sqlgen`'s checks call it.

/// The first rows of a result. Cells are rendered as text; `None` is SQL NULL.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows existed than were read.
    pub truncated: bool,
}

impl Preview {
    /// As a small text table: header, rows, and a note when cut.
    pub fn render(&self) -> String {
        let mut out = self.columns.join(" | ");
        out.push('\n');
        for r in &self.rows {
            let cells: Vec<String> = r
                .iter()
                .map(|c| c.clone().unwrap_or_else(|| "NULL".into()))
                .map(|c| {
                    if c.chars().count() > 60 {
                        format!("{}…", c.chars().take(60).collect::<String>())
                    } else {
                        c
                    }
                })
                .collect();
            out.push_str(&cells.join(" | "));
            out.push('\n');
        }
        out.push_str(&format!(
            "({} row{}{})",
            self.rows.len(),
            if self.rows.len() == 1 { "" } else { "s" },
            if self.truncated {
                ", more not shown"
            } else {
                ""
            }
        ));
        out
    }

    /// The first row as whole numbers (a count probe's answer).
    pub fn single_number(&self) -> Option<Vec<i64>> {
        let row = self.rows.first()?;
        row.iter()
            .map(|c| {
                c.as_deref()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|f| f as i64)
            })
            .collect()
    }
}

/// Runs read-only SQL against the database a query is meant for.
pub trait Probe {
    /// At most `max_rows` rows of `sql`; `Err` holds the engine's message.
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String>;
}
