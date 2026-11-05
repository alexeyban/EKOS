//! RFC 0240 §4: the fixture suite end to end — prepare → run → score → compare → export — with
//! the scripted model, plus the leakage, budget and contamination guards. The EKOS pack is built
//! here from the database's own catalogue; `crates/cli/tests/bench.rs` builds it from a real
//! compiled ledger.

use ekos_evals::bench::context::{
    DbPack, PackColumn, PackTable, Profile, SchemaInfo, ValueIndex, sqlgen_schema,
};
use ekos_evals::bench::fixture::{self, ScriptedLlm};
use ekos_evals::bench::report::{self, BenchReport, RunMeta};
use ekos_evals::bench::sqlexec::Db;
use ekos_evals::bench::suite::{Suite, SuiteDirs};
use ekos_evals::bench::{
    self, BenchAdapter, DbHandle, ExperimentConfig, ExportTarget, PrepareCtx, RunCtx, Split,
};
use ekos_recovery::{CachedLlmProvider, LlmProvider};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Fixture {
    _tmp: tempfile::TempDir,
    suite_dir: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let suite_dir = tmp.path().join("fixture");
    fixture::write_suite(&suite_dir).unwrap();
    Fixture {
        _tmp: tmp,
        suite_dir,
    }
}

fn adapter(dir: &Path) -> Box<dyn BenchAdapter> {
    bench::adapter_for(Suite::load(dir).unwrap(), SuiteDirs::new(dir)).unwrap()
}

/// What a compiled ledger would hold, read off the database: tables, columns, foreign keys.
fn pack_from_catalogue(db: &Db, schema: &SchemaInfo) -> DbPack {
    let mut pack = DbPack::default();
    for t in &schema.table_names {
        pack.tables.push(PackTable {
            name: t.clone(),
            description: None,
            columns: schema.tables[&t.to_lowercase()]
                .iter()
                .map(|(c, ty, pk)| PackColumn {
                    name: c.clone(),
                    data_type: ty.clone(),
                    primary_key: *pk > 0,
                    description: None,
                })
                .collect(),
        });
        let mut stmt = db
            .connection()
            .prepare(&format!("PRAGMA foreign_key_list(\"{t}\")"))
            .unwrap();
        let fks: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(3)?, r.get(2)?, r.get(4)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (from, to, to_col) in fks {
            pack.joins.push(format!("{t}.{from} = {to}.{to_col}"));
        }
    }
    pack
}

fn open_dbs(adapter: &dyn BenchAdapter, dirs: &SuiteDirs) -> BTreeMap<String, DbHandle> {
    let mut dbs = BTreeMap::new();
    for db_id in ["shop", "school"] {
        let db = Db::open(&adapter.database_path(db_id)).unwrap();
        let schema = SchemaInfo::read(&db).unwrap();
        let pack = pack_from_catalogue(&db, &schema);
        let values: ValueIndex = serde_json::from_str(
            &std::fs::read_to_string(dirs.values().join(format!("{db_id}.json"))).unwrap(),
        )
        .unwrap();
        let profile: Profile = serde_json::from_str(
            &std::fs::read_to_string(dirs.values().join(format!("{db_id}.profile.json"))).unwrap(),
        )
        .unwrap();
        let sql_schema =
            sqlgen_schema(&db, &schema, Some(&pack), Some(&profile), Some(&values)).unwrap();
        dbs.insert(
            db_id.to_string(),
            DbHandle {
                db,
                schema,
                pack: Some(pack),
                values: Some(values),
                profile: Some(profile),
                fingerprint: Some(format!("fp-{db_id}")),
                sql_schema: Some(sql_schema),
            },
        );
    }
    dbs
}

async fn run_cell(
    dir: &Path,
    cell: &str,
    split: Split,
    llm: &dyn LlmProvider,
    earlier: &[(PathBuf, BenchReport)],
) -> BenchReport {
    let cfg = ExperimentConfig::load(&dir.join("configs").join(format!("{cell}.toml"))).unwrap();
    run_config(dir, &cfg, split, llm, earlier).await
}

async fn run_config(
    dir: &Path,
    cfg: &ExperimentConfig,
    split: Split,
    llm: &dyn LlmProvider,
    earlier: &[(PathBuf, BenchReport)],
) -> BenchReport {
    let adapter = adapter(dir);
    let dirs = SuiteDirs::new(dir);
    let suite = Suite::load(dir).unwrap();
    let dbs = open_dbs(adapter.as_ref(), &dirs);
    let rt = RunCtx {
        llm,
        dbs: &dbs,
        price: Some((1.0, 2.0)),
    };
    let planned = adapter.scenarios(split).unwrap();
    let hold = adapter.scenarios(Split::Hold).unwrap();
    let hold_hash = bench::split::ids_hash(&hold);
    let outcome = bench::run(adapter.as_ref(), &planned, cfg, &rt, &mut |_, _, _| {}).await;
    BenchReport::assemble(
        RunMeta {
            suite: &suite,
            config: cfg,
            split,
            planned: &planned,
            hold_ids_hash: &hold_hash,
            model: "scripted".into(),
            ledger_fingerprints: dbs
                .iter()
                .map(|(k, v)| (k.clone(), v.fingerprint.clone().unwrap()))
                .collect(),
            started_at: "2026-11-03T10:00:00Z".into(),
            finished_at: "2026-11-03T10:00:01Z".into(),
            price: Some((1.0, 2.0)),
            earlier,
        },
        outcome,
    )
}

#[tokio::test]
async fn the_fixture_suite_runs_end_to_end() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    let specs = a
        .prepare(&PrepareCtx {
            dirs: &SuiteDirs::new(&f.suite_dir),
            force: false,
        })
        .unwrap();
    assert_eq!(specs.len(), 2);
    let shop = specs.iter().find(|s| s.db_id == "shop").unwrap();
    let schema = std::fs::read_to_string(shop.source_dir.join("schema.sql")).unwrap();
    assert!(schema.contains("CREATE TABLE \"orders\""), "{schema}");
    assert!(schema.contains("FOREIGN KEY (\"customer_id\") REFERENCES \"customers\" (\"id\")"));
    assert!(
        schema.contains("COMMENT ON COLUMN \"customers\".\"segment\" IS 'customer segment; values: retail: a private person; corporate: a company';"),
        "{schema}"
    );
    // Prepare is idempotent.
    let again = a
        .prepare(&PrepareCtx {
            dirs: &SuiteDirs::new(&f.suite_dir),
            force: false,
        })
        .unwrap();
    assert_eq!(again[0].source_digest, specs[0].source_digest);

    let mut ex = BTreeMap::new();
    let mut reports = BTreeMap::new();
    for cell in ["a", "b", "c", "d", "c-prime"] {
        let llm = ScriptedLlm::new();
        let r = run_cell(&f.suite_dir, cell, Split::All, &llm, &[]).await;
        assert_eq!(r.n, 10);
        assert_eq!(r.planned, 10);
        assert!(r.budget_stopped.is_none());
        ex.insert(cell, r.metrics.ex.mean.round() as i64);
        reports.insert(cell, r);
    }
    // Values only the evidence or value lookup shows decide three questions.
    assert_eq!(
        ex,
        BTreeMap::from([("a", 60), ("b", 80), ("c", 90), ("c-prime", 60), ("d", 90)])
    );
    // RFC 0241: profiles show the example values ('retail', 'Maths', 'Gryff'), so every
    // profile cell gets the three right without value lookup.
    for cell in ["e", "f", "g", "h"] {
        let r = run_cell(&f.suite_dir, cell, Split::All, &ScriptedLlm::new(), &[]).await;
        assert_eq!(r.metrics.ex.mean.round(), 90.0, "cell {cell}");
        assert_eq!(
            r.metrics.categories["wrong_values"], 1,
            "cell {cell}: question 9"
        );
    }
    let h = run_cell(&f.suite_dir, "h", Split::All, &ScriptedLlm::new(), &[]).await;
    assert!(
        h.questions.iter().all(|q| q.prediction.calls >= 3),
        "three candidates each"
    );
    let a_cats = &reports["a"].metrics.categories;
    assert_eq!(a_cats["correct"], 6);
    assert_eq!(
        a_cats["wrong_values"] + a_cats["wrong_rows"],
        4,
        "{a_cats:?}"
    );

    let c = &reports["c"];
    let q4 = c.questions.iter().find(|q| q.id == "4").unwrap();
    assert_eq!(q4.prediction.repairs, 1, "the first answer did not run");
    assert!(q4.score.ex);
    let q9 = c.questions.iter().find(|q| q.id == "9").unwrap();
    assert!(!q9.score.ex && q9.score.executed && q9.score.grounded);
    assert!(q9.score.soft_f1 < 1.0);
    assert_eq!(c.metrics.grounding_rate, 100.0);
    assert_eq!(c.metrics.schema_recall, Some(100.0));
    assert_eq!(c.metrics.join_recall, Some(100.0));
    assert!(c.metrics.value_hit_rate.unwrap() > 0.0);
    assert!(reports["a"].metrics.value_hit_rate.is_none());
    assert!(c.metrics.cost_known && c.metrics.cost_usd > 0.0);
    assert_eq!(c.metrics.by_difficulty["simple"].n, 4);
    assert!(c.render().contains("n = 10 of 10 planned"));
    assert!(reports["b"].oracle_knowledge && !c.oracle_knowledge);

    let cmp = report::compare(c, &reports["a"]);
    assert_eq!(cmp.paired, 10);
    assert_eq!(cmp.only_a_correct, vec!["1", "6", "7"]);
    assert!(cmp.only_b_correct.is_empty());
    assert!((cmp.diff.mean - 30.0).abs() < 1e-9);
    let same = report::compare(c, c);
    assert_eq!(same.diff.mean, 0.0);
    assert!(!same.significant);

    // Save, list and export.
    let reports_dir = f.suite_dir.join("reports");
    let path = c.save(&reports_dir).unwrap();
    assert_eq!(&BenchReport::load(&path).unwrap(), c);
    assert_eq!(report::load_all(&reports_dir).len(), 1);
    let out = f.suite_dir.join("export");
    let predict = a.export(c, ExportTarget::BirdSubmission, &out).unwrap();
    let text = std::fs::read_to_string(&predict).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed.as_object().unwrap().len(), 10);
    assert!(text.find("\"0\"").unwrap() < text.find("\"9\"").unwrap());
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(predict.with_extension("metadata.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta["oracle_knowledge"], false);
}

#[tokio::test]
async fn a_budget_refusal_stops_the_run_and_the_report_says_so() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    a.prepare(&PrepareCtx {
        dirs: &SuiteDirs::new(&f.suite_dir),
        force: false,
    })
    .unwrap();
    let llm = ScriptedLlm {
        budget_calls: Some(3),
        ..ScriptedLlm::new()
    };
    let r = run_cell(&f.suite_dir, "a", Split::All, &llm, &[]).await;
    assert_eq!(r.n, 3);
    assert_eq!(r.planned, 10);
    assert!(r.budget_stopped.is_some());
    assert!(r.render().contains("STOPPED EARLY by the budget"));
}

#[test]
fn a_gold_file_planted_in_a_project_fails_prepare() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    let dirs = SuiteDirs::new(&f.suite_dir);
    let ctx = PrepareCtx {
        dirs: &dirs,
        force: false,
    };
    a.prepare(&ctx).unwrap();
    let planted = dirs.projects().join("shop").join("gold.sql");
    std::fs::write(&planted, fixture::QUESTIONS[0].5).unwrap();
    let err = a.prepare(&ctx).unwrap_err().to_string();
    assert!(err.contains("gold.sql"), "{err}");
    std::fs::remove_file(&planted).unwrap();
    a.prepare(&ctx).unwrap();
}

#[test]
fn gold_sql_smuggled_through_a_dictionary_fails_prepare() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    let csv = f
        .suite_dir
        .join("databases/school/database_description/scores.csv");
    let mut text = std::fs::read_to_string(&csv).unwrap();
    text.push_str(&format!(
        "score,,\"{}\",integer,\n",
        fixture::QUESTIONS[9].5
    ));
    std::fs::write(&csv, text).unwrap();
    let err = a
        .prepare(&PrepareCtx {
            dirs: &SuiteDirs::new(&f.suite_dir),
            force: false,
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("leakage") && err.contains("question 9"),
        "{err}"
    );
}

#[tokio::test]
async fn changing_a_cell_after_its_hold_out_run_marks_the_full_run_contaminated() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    a.prepare(&PrepareCtx {
        dirs: &SuiteDirs::new(&f.suite_dir),
        force: false,
    })
    .unwrap();
    let hold = run_cell(&f.suite_dir, "c", Split::Hold, &ScriptedLlm::new(), &[]).await;
    assert!(!hold.tuned_on && hold.contaminated.is_none());
    let earlier = vec![(PathBuf::from("hold.json"), hold)];
    // Same configuration on the full set: clean.
    let full = run_cell(&f.suite_dir, "c", Split::All, &ScriptedLlm::new(), &earlier).await;
    assert!(full.contaminated.is_none());
    // The cell changed after its hold-out result was seen.
    let cfg_path = f.suite_dir.join("configs/c.toml");
    let text = std::fs::read_to_string(&cfg_path)
        .unwrap()
        .replace("max-tokens = 512", "max-tokens = 600");
    std::fs::write(&cfg_path, text).unwrap();
    let full = run_cell(&f.suite_dir, "c", Split::All, &ScriptedLlm::new(), &earlier).await;
    assert!(full.contaminated.is_some());
    assert!(full.render().contains("CONTAMINATED"));
    let tune = run_cell(
        &f.suite_dir,
        "c",
        Split::Tune,
        &ScriptedLlm::new(),
        &earlier,
    )
    .await;
    assert!(tune.tuned_on && tune.contaminated.is_none());
}

#[tokio::test]
async fn result_checks_send_an_empty_or_null_result_back() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    a.prepare(&PrepareCtx {
        dirs: &SuiteDirs::new(&f.suite_dir),
        force: false,
    })
    .unwrap();
    // Cell G without profiles or values: only the result check can rescue questions 1, 6, 7.
    let mut cfg = ExperimentConfig::load(&f.suite_dir.join("configs/g.toml")).unwrap();
    cfg.profiles = false;
    cfg.value_lookup = false;
    let r = run_config(&f.suite_dir, &cfg, Split::All, &ScriptedLlm::new(), &[]).await;
    assert_eq!(r.metrics.ex.mean.round(), 90.0);
    for id in ["1", "6", "7"] {
        let q = r.questions.iter().find(|q| q.id == id).unwrap();
        assert_eq!(q.prediction.repairs, 1, "question {id}");
        assert!(q.score.ex, "question {id}");
    }
    cfg.result_checks = false;
    let r = run_config(&f.suite_dir, &cfg, Split::All, &ScriptedLlm::new(), &[]).await;
    assert_eq!(r.metrics.ex.mean.round(), 60.0);
}

#[tokio::test]
async fn rfc_0242_checks_rescue_value_spellings_in_the_loop_and_the_agent() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    a.prepare(&PrepareCtx {
        dirs: &SuiteDirs::new(&f.suite_dir),
        force: false,
    })
    .unwrap();
    for cell in ["j", "k"] {
        // Without profiles or value lookup the prompt never shows 'retail', 'Maths', 'Gryff';
        // only a finding (L004 offers the stored spelling) can rescue questions 1, 6, 7.
        let mut cfg =
            ExperimentConfig::load(&f.suite_dir.join(format!("configs/{cell}.toml"))).unwrap();
        cfg.profiles = false;
        cfg.value_lookup = false;
        let r = run_config(&f.suite_dir, &cfg, Split::All, &ScriptedLlm::new(), &[]).await;
        assert_eq!(r.metrics.ex.mean.round(), 90.0, "cell {cell}");
        for id in ["1", "6", "7"] {
            let q = r.questions.iter().find(|q| q.id == id).unwrap();
            assert_eq!(q.prediction.turns, 2, "cell {cell} question {id}");
            assert!(q.prediction.findings.contains(&"L004".to_string()));
            assert!(q.prediction.open_findings.is_empty());
            assert!(q.score.ex, "cell {cell} question {id}");
        }
        let q0 = r.questions.iter().find(|q| q.id == "0").unwrap();
        assert_eq!(q0.prediction.turns, 1, "a clean answer is accepted at once");
        assert!(r.metrics.turns >= 13);
        // Question 9 (score > 80 vs >= 80) is wrong and nothing can see it.
        assert_eq!(r.metrics.categories["wrong_values"], 1);
    }
    // Existing cells keep their configuration hash: new fields at their defaults are not written.
    let g = ExperimentConfig::load(&f.suite_dir.join("configs/g.toml")).unwrap();
    let text = toml::to_string(&g).unwrap();
    assert!(
        !text.contains("strategy") && !text.contains("max-turns") && !text.contains("\nchecks")
    );
}

/// RFC 0244: a test-set question (no gold SQL) is predicted and left unscored, and a run resumed
/// from a checkpoint asks only the questions it does not hold.
#[tokio::test]
async fn rfc_0244_predict_only_questions_and_resume() {
    let f = fixture();
    let adapter = adapter(&f.suite_dir);
    adapter
        .prepare(&PrepareCtx {
            dirs: &SuiteDirs::new(&f.suite_dir),
            force: false,
        })
        .unwrap();
    let dirs = SuiteDirs::new(&f.suite_dir);
    let dbs = open_dbs(adapter.as_ref(), &dirs);
    let cfg = ExperimentConfig::load(&f.suite_dir.join("configs/a.toml")).unwrap();
    let mut planned = adapter.scenarios(Split::All).unwrap();
    planned[0].gold_sql = String::new();
    let llm = ScriptedLlm::new();
    let rt = RunCtx {
        llm: &llm,
        dbs: &dbs,
        price: None,
    };
    let first = bench::run(adapter.as_ref(), &planned, &cfg, &rt, &mut |_, _, _| {}).await;
    let q0 = &first.results[0];
    assert!(
        q0.prediction.sql.is_some(),
        "a test question is still answered"
    );
    assert_eq!(q0.score.category, bench::Category::Unscored);
    assert!(q0.score.gold_error.is_none());
    let asked = llm.calls.load(std::sync::atomic::Ordering::SeqCst);

    // Resume with the first three answered: only the rest are asked, and the outcome is whole.
    let done = first.results[..3].to_vec();
    let llm2 = ScriptedLlm::new();
    let rt2 = RunCtx {
        llm: &llm2,
        dbs: &dbs,
        price: None,
    };
    let mut fresh = 0;
    let resumed = bench::run_from(
        adapter.as_ref(),
        &planned,
        &cfg,
        &rt2,
        done,
        &mut |_, _, _| fresh += 1,
    )
    .await;
    assert_eq!(resumed.results.len(), planned.len());
    assert_eq!(fresh, planned.len() - 3);
    assert!(llm2.calls.load(std::sync::atomic::Ordering::SeqCst) < asked);
    let ids: Vec<&str> = resumed.results.iter().map(|r| r.id.as_str()).collect();
    let want: Vec<&str> = planned.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, want, "results keep the questions' order");
}

#[tokio::test]
async fn cache_served_calls_are_counted_and_not_billed() {
    let f = fixture();
    let a = adapter(&f.suite_dir);
    a.prepare(&PrepareCtx {
        dirs: &SuiteDirs::new(&f.suite_dir),
        force: false,
    })
    .unwrap();
    let cache = f.suite_dir.join("llm-cache");
    let first = run_cell(
        &f.suite_dir,
        "a",
        Split::All,
        &CachedLlmProvider::new(ScriptedLlm::new(), &cache),
        &[],
    )
    .await;
    assert_eq!(first.metrics.cached_calls, 0);
    assert!(first.metrics.cost_usd > 0.0);
    assert_eq!(
        first.metrics.billed_input_tokens,
        first.metrics.input_tokens
    );
    let second = run_cell(
        &f.suite_dir,
        "a",
        Split::All,
        &CachedLlmProvider::new(ScriptedLlm::new(), &cache),
        &[],
    )
    .await;
    assert_eq!(second.metrics.cached_calls, second.metrics.calls);
    assert_eq!(second.metrics.billed_input_tokens, 0);
    assert_eq!(second.metrics.cost_usd, 0.0);
    assert_eq!(second.metrics.ex, first.metrics.ex);
    assert!(second.render().contains("from the LLM cache"));
}
