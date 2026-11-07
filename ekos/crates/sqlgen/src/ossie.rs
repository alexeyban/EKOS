//! RFC 0246: what an Apache Ossie semantic model adds to the schema the checks and the agent see —
//! synonyms, descriptions where the schema has none, time axes, stated unique keys, joins no
//! foreign key declares, and metric definitions. The model never overrides the schema: a
//! description the schema states is kept, and a declared join only adds a path.

use crate::schema::{ForeignKey, Metric, Schema};
use ekos_ossie::knowledge::OssieKnowledge;

fn push_unique(into: &mut Vec<String>, from: &[String]) {
    for s in from {
        if !into.iter().any(|x| x.eq_ignore_ascii_case(s)) {
            into.push(s.clone());
        }
    }
}

impl Schema {
    /// This schema with what the current Ossie models state about its tables.
    pub fn with_ossie(mut self, k: &OssieKnowledge) -> Schema {
        for tf in &k.tables {
            let Some(i) = self.tables.iter().position(|t| {
                t.name.eq_ignore_ascii_case(&tf.table)
                    || t.qualified().eq_ignore_ascii_case(&tf.table)
            }) else {
                continue;
            };
            let t = &mut self.tables[i];
            push_unique(&mut t.synonyms, &tf.synonyms);
            if t.description.is_none() {
                t.description = tf.description.clone();
            }
            for key in std::iter::once(&tf.primary_key).chain(&tf.unique_keys) {
                if !key.is_empty() && !t.unique_keys.contains(key) {
                    t.unique_keys.push(key.clone());
                }
            }
            for cf in &tf.columns {
                if let Some(c) = t
                    .columns
                    .iter_mut()
                    .find(|c| c.name.eq_ignore_ascii_case(&cf.column))
                {
                    push_unique(&mut c.synonyms, &cf.synonyms);
                    c.is_time |= cf.is_time;
                    if c.description.is_none() {
                        c.description = cf.description.clone();
                    }
                }
            }
        }
        for j in &k.joins {
            for (fc, tc) in j.from_columns.iter().zip(&j.to_columns) {
                let fk = ForeignKey {
                    from_table: j.from_table.clone(),
                    from_column: fc.clone(),
                    to_table: j.to_table.clone(),
                    to_column: tc.clone(),
                };
                if !self.is_foreign_key(
                    (&fk.from_table, &fk.from_column),
                    (&fk.to_table, &fk.to_column),
                ) {
                    self.declared_joins.push(fk);
                }
            }
        }
        self.declared_joins.sort();
        self.declared_joins.dedup();
        for m in &k.metrics {
            if !self
                .metrics
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&m.name))
            {
                self.metrics.push(Metric {
                    name: m.name.clone(),
                    sql: m.sql.clone(),
                    tables: m.tables.clone(),
                    description: m.description.clone(),
                    synonyms: m.synonyms.clone(),
                    origin: m.origin.clone(),
                });
            }
        }
        self.metrics.sort_by(|a, b| a.name.cmp(&b.name));
        self
    }

    /// A defined metric by name or synonym (case-insensitive, `_` and spaces alike).
    pub fn metric(&self, name: &str) -> Option<&Metric> {
        let norm = |s: &str| s.trim().to_lowercase().replace([' ', '-'], "_");
        let n = norm(name);
        self.metrics
            .iter()
            .find(|m| norm(&m.name) == n)
            .or_else(|| {
                self.metrics
                    .iter()
                    .find(|m| m.synonyms.iter().any(|s| norm(s) == n))
            })
    }

    /// The `metric` tool's answer: the definition, or the defined names when none matches.
    pub fn describe_metric(&self, name: &str) -> String {
        match self.metric(name) {
            Some(m) => format!(
                "metric {}\n  reads: {}\n  Use this definition when the question asks for {}.",
                m.render(),
                m.tables.join(", "),
                m.name
            ),
            None if self.metrics.is_empty() => "no metrics are defined for this database".into(),
            None => format!(
                "no metric named {name}; defined: {}",
                self.metrics
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{Column, Schema, Table};
    use ekos_kir::KirId;
    use ekos_ossie::knowledge::{ColumnFacts, Join, Metric, OssieKnowledge, TableFacts};

    fn schema() -> Schema {
        let col = |n: &str| Column {
            name: n.into(),
            data_type: "integer".into(),
            ..Default::default()
        };
        Schema {
            tables: vec![
                Table {
                    name: "store_sales".into(),
                    columns: vec![
                        col("ss_customer_sk"),
                        col("ss_quantity"),
                        col("ss_sold_date"),
                    ],
                    ..Default::default()
                },
                Table {
                    name: "customer".into(),
                    description: Some("from the DDL".into()),
                    columns: vec![col("c_customer_sk"), col("c_email")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn knowledge() -> OssieKnowledge {
        OssieKnowledge {
            tables: vec![
                TableFacts {
                    table_id: KirId::new(),
                    table: "customer".into(),
                    synonyms: vec!["shoppers".into()],
                    description: Some("from the model".into()),
                    primary_key: vec!["c_customer_sk".into()],
                    columns: vec![ColumnFacts {
                        column: "c_email".into(),
                        description: Some("contact address".into()),
                        synonyms: vec!["e-mail".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                TableFacts {
                    table_id: KirId::new(),
                    table: "store_sales".into(),
                    columns: vec![ColumnFacts {
                        column: "ss_sold_date".into(),
                        is_time: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            joins: vec![Join {
                name: "sales_to_customer".into(),
                from_table: "store_sales".into(),
                from_columns: vec!["ss_customer_sk".into()],
                to_table: "customer".into(),
                to_columns: vec!["c_customer_sk".into()],
                origin: "ossie:m.yaml".into(),
            }],
            metrics: vec![Metric {
                ekos_id: "m1".into(),
                name: "total_quantity".into(),
                scope: "dataset".into(),
                sql: "SUM(store_sales.ss_quantity)".into(),
                tables: vec!["store_sales".into()],
                description: Some("Units sold".into()),
                synonyms: vec!["units sold".into()],
                datatype: None,
                origin: "ossie:m.yaml".into(),
                line: None,
            }],
            skipped: vec![],
        }
    }

    #[test]
    fn the_model_adds_without_overriding() {
        let s = schema().with_ossie(&knowledge());
        let customer = s.table("customer").unwrap();
        assert_eq!(
            customer.description.as_deref(),
            Some("from the DDL"),
            "the schema wins"
        );
        assert_eq!(customer.synonyms, ["shoppers"]);
        assert_eq!(customer.unique_keys, [vec!["c_customer_sk".to_string()]]);
        assert_eq!(
            customer.column("c_email").unwrap().description.as_deref(),
            Some("contact address"),
            "an empty description is filled"
        );
        assert!(s.column("store_sales", "ss_sold_date").unwrap().is_time);
        // A join no foreign key declares becomes a path, marked as declared.
        assert_eq!(
            s.join_path("store_sales", "customer").unwrap(),
            [
                "store_sales.ss_customer_sk = customer.c_customer_sk (declared in the semantic model)"
            ]
        );
        assert!(s.is_foreign_key(
            ("customer", "c_customer_sk"),
            ("store_sales", "ss_customer_sk")
        ));
    }

    #[test]
    fn synonyms_and_metrics_are_searchable_and_described() {
        let s = schema().with_ossie(&knowledge());
        assert!(
            s.search("shoppers")
                .iter()
                .any(|l| l.starts_with("table customer"))
        );
        assert!(
            s.search("e-mail")
                .iter()
                .any(|l| l.contains("customer.c_email"))
        );
        assert!(
            s.search("units sold")
                .iter()
                .any(|l| l.starts_with("metric total_quantity"))
        );
        let d = s.describe("store_sales").unwrap();
        assert!(
            d.contains("metric total_quantity = SUM(store_sales.ss_quantity)"),
            "{d}"
        );
        assert!(d.contains("time axis"), "{d}");
        assert!(
            s.describe_metric("Units Sold")
                .contains("SUM(store_sales.ss_quantity)")
        );
        assert!(
            s.describe_metric("revenue")
                .contains("defined: total_quantity")
        );
    }

    #[test]
    fn a_metric_across_tables_is_listed_once_in_the_prompt() {
        let mut k = knowledge();
        k.metrics.push(Metric {
            ekos_id: "m2".into(),
            name: "per_customer".into(),
            scope: "model".into(),
            sql: "SUM(store_sales.ss_quantity) / COUNT(DISTINCT customer.c_customer_sk)".into(),
            tables: vec!["customer".into(), "store_sales".into()],
            description: None,
            synonyms: vec![],
            datatype: None,
            origin: "ossie:m.yaml".into(),
            line: None,
        });
        let s = schema().with_ossie(&k);
        let all: Vec<&crate::Table> = s.tables.iter().collect();
        let text = s.render(&all);
        assert_eq!(text.matches("metric per_customer").count(), 1, "{text}");
        let one = s.render(&[s.table("customer").unwrap()]);
        assert!(
            !one.contains("per_customer"),
            "not all its tables are shown"
        );
    }

    #[test]
    fn a_metric_word_picks_its_table_among_many() {
        let mut s = schema();
        for i in 0..10 {
            s.tables.push(Table {
                name: format!("other_{i}"),
                ..Default::default()
            });
        }
        let s = s.with_ossie(&knowledge());
        let picked: Vec<&str> = s
            .relevant("how many units sold last month", 3)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(picked.contains(&"store_sales"), "{picked:?}");
    }
}
