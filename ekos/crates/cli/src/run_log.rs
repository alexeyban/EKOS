//! RFC 0220 — the structured run log of a pipeline command.
//!
//! A `tracing` layer beside the terminal one. It writes every [`ekos_common::run_log`] event as one
//! JSON line (`stage`, `msg`, the event's counts, `cmd`, `t_ms`), copies every warning and error any
//! crate logs (redacted, RFC 0043), counts model calls, and ends the run with an `llm` line and a
//! `run finished` line. Each line is flushed as it is written: the console reads the file live.

use crate::commands::catalogue::Class;
use anyhow::Result;
use ekos_common::redaction::{RedactionConfig, redact};
use ekos_common::run_log::{CACHE_HIT, COMPLETION, EMBEDDING, MODEL_CALL_TARGET, TARGET};
use ekos_compiler_core::EkosConfig;
use serde_json::Value;
use std::fmt::Debug;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// The commands that write a run log.
pub const COMMANDS: &[&str] = &["build", "recover", "resolve", "compile", "commit"];

/// Line keys a producer's counts may not take.
const RESERVED: &[&str] = &["stage", "msg", "cmd", "t_ms"];

/// Longest message or error text copied into a line.
const MAX_TEXT: usize = 500;

/// Warnings and errors copied verbatim; the rest are counted by target on one closing line (a
/// large compile logs one warning per dangling edge — 139,000 on iDempiere).
const MAX_COPIED: u64 = 200;

/// One command's run log. Cloning shares the sink and the counters.
#[derive(Clone)]
pub struct RunLog {
    inner: Arc<Inner>,
}

struct Inner {
    cmd: String,
    started: Instant,
    out: Mutex<Box<dyn Write + Send>>,
    redaction: RedactionConfig,
    completions: AtomicU64,
    cache_hits: AtomicU64,
    embeddings: AtomicU64,
    warnings: AtomicU64,
    /// Warnings beyond [`MAX_COPIED`], by target.
    not_copied: Mutex<std::collections::BTreeMap<String, u64>>,
}

impl RunLog {
    /// A run log appended to `path` (its directory is created).
    pub fn open(path: &Path, cmd: &str, redaction: RedactionConfig) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self::to_writer(Box::new(file), cmd, redaction))
    }

    /// A run log written to `out`.
    pub fn to_writer(out: Box<dyn Write + Send>, cmd: &str, redaction: RedactionConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                cmd: cmd.to_string(),
                started: Instant::now(),
                out: Mutex::new(out),
                redaction,
                completions: AtomicU64::new(0),
                cache_hits: AtomicU64::new(0),
                embeddings: AtomicU64::new(0),
                warnings: AtomicU64::new(0),
                not_copied: Mutex::new(std::collections::BTreeMap::new()),
            }),
        }
    }

    /// The layer to install; it filters to the events it records.
    pub fn layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        RunLogLayer { log: self.clone() }.with_filter(tracing_subscriber::filter::filter_fn(
            |meta| {
                meta.target() == TARGET
                    || meta.target() == MODEL_CALL_TARGET
                    || *meta.level() <= Level::WARN
            },
        ))
    }

    /// The first line: which command, which EKOS.
    pub fn started(&self) {
        self.write(
            "run",
            "started",
            &[("version".into(), env!("CARGO_PKG_VERSION").into())],
        );
    }

    /// The last two lines: the model calls this process made, then the outcome.
    pub fn finish(&self, outcome: &Result<()>) {
        let i = &self.inner;
        let n = |c: &AtomicU64| Value::from(c.load(Ordering::Relaxed));
        let not_copied =
            std::mem::take(&mut *i.not_copied.lock().unwrap_or_else(|p| p.into_inner()));
        if !not_copied.is_empty() {
            let total: u64 = not_copied.values().sum();
            self.write(
                "warning",
                &format!("{total} more warning(s) not copied"),
                &[("by_target".into(), serde_json::json!(not_copied))],
            );
        }
        self.write(
            "llm",
            "calls",
            &[
                ("count".into(), n(&i.completions)),
                ("cache_hits".into(), n(&i.cache_hits)),
                ("embeddings".into(), n(&i.embeddings)),
            ],
        );
        let mut fields = vec![
            ("ok".to_string(), Value::from(outcome.is_ok())),
            (
                "ms".into(),
                Value::from(i.started.elapsed().as_millis() as u64),
            ),
            ("warnings".into(), n(&i.warnings)),
        ];
        if let Err(e) = outcome {
            fields.push(("error".into(), self.text(&format!("{e:#}")).into()));
        }
        self.write("run", "finished", &fields);
    }

    fn text(&self, s: &str) -> String {
        let s = redact(s, &self.inner.redaction);
        if s.chars().count() > MAX_TEXT {
            s.chars().take(MAX_TEXT).collect::<String>() + "…"
        } else {
            s
        }
    }

    fn write(&self, stage: &str, msg: &str, fields: &[(String, Value)]) {
        let body: Vec<String> = fields
            .iter()
            .filter(|(k, _)| !RESERVED.contains(&k.as_str()))
            .map(|(k, v)| format!("{}:{v}", Value::from(k.as_str())))
            .collect();
        self.write_raw(stage, msg, &body.join(","));
    }

    /// `counts` is the inside of a JSON object (`"a":1,"b":2`) or empty.
    fn write_raw(&self, stage: &str, msg: &str, counts: &str) {
        let mut line = format!(
            "{{\"stage\":{},\"msg\":{}",
            Value::from(stage),
            Value::from(msg)
        );
        if !counts.is_empty() {
            line.push(',');
            line.push_str(counts);
        }
        line.push_str(&format!(
            ",\"cmd\":{},\"t_ms\":{}}}\n",
            Value::from(self.inner.cmd.as_str()),
            self.inner.started.elapsed().as_millis()
        ));
        let mut out = self.inner.out.lock().unwrap_or_else(|p| p.into_inner());
        // A run log that cannot be written must not fail the command.
        let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
    }

    fn record_event(&self, event: &Event<'_>) {
        let mut f = Fields::default();
        event.record(&mut f);
        let target = event.metadata().target();
        if target == MODEL_CALL_TARGET {
            let counter = match f.kind.as_deref() {
                Some(COMPLETION) => &self.inner.completions,
                Some(CACHE_HIT) => &self.inner.cache_hits,
                Some(EMBEDDING) => &self.inner.embeddings,
                _ => return,
            };
            counter.fetch_add(1, Ordering::Relaxed);
        } else if target == TARGET {
            let stage = f.stage.unwrap_or_else(|| "unknown".into());
            let msg = f.msg.unwrap_or_default();
            self.write_raw(&stage, &msg, &counts_of(f.data.as_deref()));
        } else {
            let seen = self.inner.warnings.fetch_add(1, Ordering::Relaxed);
            if seen >= MAX_COPIED {
                let mut rest = self
                    .inner
                    .not_copied
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                *rest.entry(target.to_string()).or_default() += 1;
                return;
            }
            let level = if *event.metadata().level() == Level::ERROR {
                "error"
            } else {
                "warning"
            };
            let mut fields = vec![("target".to_string(), Value::from(target))];
            for (k, v) in f.rest {
                let v = match v {
                    Value::String(s) => Value::from(self.text(&s)),
                    other => other,
                };
                fields.push((k, v));
            }
            let msg = self.text(&f.msg.unwrap_or_default());
            self.write(level, &msg, &fields);
        }
    }
}

/// The inside of the `data` object, keeping the producer's key order; reserved keys dropped.
fn counts_of(data: Option<&str>) -> String {
    let Some(raw) = data else {
        return String::new();
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) if map.keys().all(|k| !RESERVED.contains(&k.as_str())) => {
            let raw = raw.trim();
            raw[1..raw.len() - 1].to_string()
        }
        Ok(Value::Object(map)) => map
            .iter()
            .filter(|(k, _)| !RESERVED.contains(&k.as_str()))
            .map(|(k, v)| format!("{}:{v}", Value::from(k.as_str())))
            .collect::<Vec<_>>()
            .join(","),
        Ok(Value::Null) | Err(_) => String::new(),
        Ok(other) => format!("\"data\":{other}"),
    }
}

struct RunLogLayer {
    log: RunLog,
}

impl<S: Subscriber> Layer<S> for RunLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.log.record_event(event);
    }
}

#[derive(Default)]
struct Fields {
    stage: Option<String>,
    msg: Option<String>,
    data: Option<String>,
    kind: Option<String>,
    rest: Vec<(String, Value)>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "stage" => self.stage = Some(value.into()),
            "data" => self.data = Some(value.into()),
            "kind" => self.kind = Some(value.into()),
            "message" => self.msg = Some(value.into()),
            name => self.rest.push((name.into(), value.into())),
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        let text = format!("{value:?}");
        if field.name() == "message" {
            self.msg = Some(text);
        } else {
            self.record_str(field, &text);
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.rest.push((field.name().into(), value.into()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.rest.push((field.name().into(), value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.rest.push((field.name().into(), value.into()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.rest.push((field.name().into(), value.into()));
    }
}

/// Where `cmd`'s run log goes: `EKOS_RUN_LOG`, or `<state dir>/runs/<UTC stamp>-<cmd>.jsonl`.
pub fn path_for(config: &EkosConfig, cwd: &Path, cmd: &str, env: Option<PathBuf>) -> PathBuf {
    env.unwrap_or_else(|| {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        config
            .ekos_dir(cwd)
            .join("runs")
            .join(format!("{stamp}-{cmd}.jsonl"))
    })
}

/// End `log`, when the command has one, with the command's outcome.
pub fn finish(log: Option<&RunLog>, outcome: &Result<()>) {
    if let Some(log) = log {
        log.finish(outcome);
    }
}

/// The run log for `cmd`, when it is a pipeline command, its first line written. A sink that cannot be opened is reported
/// on stderr and the command runs without one.
pub fn for_command(cmd: &str, config: &EkosConfig, cwd: &Path) -> Option<RunLog> {
    if !COMMANDS.contains(&cmd) {
        return None;
    }
    let env = std::env::var_os("EKOS_RUN_LOG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let path = path_for(config, cwd, cmd, env);
    open_started(&path, cmd, config)
}

/// RFC 0218: the run log of a command that changes something — catalogue class write, review,
/// admin or external. `path` is the command's path (`lakehouse marts build`): the log's `cmd`, and
/// its file name with dashes. Read and local commands, and commands without a class, have none.
pub fn for_class(
    path: &[String],
    class: Option<Class>,
    config: &EkosConfig,
    cwd: &Path,
) -> Option<RunLog> {
    if path.is_empty() || !class.is_some_and(changes_something) {
        return None;
    }
    let env = std::env::var_os("EKOS_RUN_LOG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let file = path_for(config, cwd, &path.join("-"), env);
    open_started(&file, &path.join(" "), config)
}

fn changes_something(class: Class) -> bool {
    matches!(
        class,
        Class::Write | Class::Review | Class::Admin | Class::External
    )
}

fn open_started(path: &Path, cmd: &str, config: &EkosConfig) -> Option<RunLog> {
    match RunLog::open(path, cmd, config.redaction_config()) {
        Ok(log) => {
            log.started();
            Some(log)
        }
        Err(e) => {
            eprintln!("warning: no run log ({}: {e})", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        String::from_utf8(buf.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Run `body` with only the run-log layer installed, then finish with `outcome`.
    fn capture(outcome: Result<()>, body: impl FnOnce()) -> Vec<Value> {
        let buf = Buffer::default();
        let log = RunLog::to_writer(Box::new(buf.clone()), "recover", RedactionConfig::default());
        let subscriber = tracing_subscriber::registry().with(log.layer());
        tracing::subscriber::with_default(subscriber, || {
            log.started();
            body();
            log.finish(&outcome);
        });
        lines(&buf)
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[derive(serde::Serialize)]
    struct Parsed {
        files: u32,
        ok: u32,
        partial: u32,
    }

    #[test]
    fn the_targets_the_layer_matches_are_the_ones_the_emitters_use() {
        // `tracing` needs literal targets; the constants must name the same strings.
        assert_eq!(TARGET, "ekos_run_log");
        assert_eq!(MODEL_CALL_TARGET, "ekos_model_call");
    }

    #[test]
    fn an_event_is_one_line_with_its_counts_in_the_producers_order() {
        let buf = Buffer::default();
        let log = RunLog::to_writer(Box::new(buf.clone()), "recover", RedactionConfig::default());
        let subscriber = tracing_subscriber::registry().with(log.layer());
        tracing::subscriber::with_default(subscriber, || {
            ekos_common::run_log::emit(
                "java.parse",
                "parsed",
                &Parsed {
                    files: 4589,
                    ok: 4500,
                    partial: 71,
                },
            );
        });
        let line = &lines(&buf)[0];
        let head = r#"{"stage":"java.parse","msg":"parsed","files":4589,"ok":4500,"partial":71,"cmd":"recover","t_ms":"#;
        assert!(line.starts_with(head), "{line}");
    }

    #[test]
    fn a_run_starts_counts_model_calls_and_ends_with_its_outcome() {
        let events = capture(Ok(()), || {
            use ekos_common::run_log::model_call;
            model_call(COMPLETION, "ollama", "llama3", true);
            model_call(COMPLETION, "zen", "m", false);
            model_call(CACHE_HIT, "cache", "m", true);
            model_call(EMBEDDING, "ollama", "nomic", true);
        });
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(events[0]["stage"], "run");
        assert_eq!(events[0]["msg"], "started");
        let llm = &events[1];
        assert_eq!(
            (
                &llm["stage"],
                &llm["count"],
                &llm["cache_hits"],
                &llm["embeddings"]
            ),
            (&"llm".into(), &2.into(), &1.into(), &1.into())
        );
        assert_eq!(events[2]["msg"], "finished");
        assert_eq!(events[2]["ok"], true);
        assert_eq!(events[2]["warnings"], 0);
    }

    #[test]
    fn a_run_without_model_calls_says_zero() {
        let events = capture(Ok(()), || {});
        assert_eq!(events[1]["count"], 0);
        assert_eq!(events[1]["embeddings"], 0);
    }

    #[test]
    fn warnings_from_any_crate_are_copied_redacted_and_counted() {
        let events = capture(Err(anyhow::anyhow!("password=hunter2xyz rejected")), || {
            tracing::warn!(target: "ekos_recovery::x", file = "a.sql", "key AKIAABCDEFGHIJKLMNOP leaked");
            tracing::info!(target: "ekos_recovery::x", "an info line is not copied");
            tracing::error!("failed");
        });
        let warning = events.iter().find(|e| e["stage"] == "warning").unwrap();
        assert_eq!(warning["target"], "ekos_recovery::x");
        assert_eq!(warning["file"], "a.sql");
        let msg = warning["msg"].as_str().unwrap();
        assert!(!msg.contains("AKIAABCDEFGHIJKLMNOP"), "{msg}");
        assert!(events.iter().any(|e| e["stage"] == "error"));
        assert!(
            !events
                .iter()
                .any(|e| e["msg"] == "an info line is not copied")
        );
        let end = events.last().unwrap();
        assert_eq!(end["ok"], false);
        assert_eq!(end["warnings"], 2);
        assert!(
            !end["error"].as_str().unwrap().contains("hunter2xyz"),
            "{end}"
        );
    }

    #[test]
    fn warnings_past_the_cap_are_counted_by_target_not_copied() {
        let events = capture(Ok(()), || {
            for _ in 0..MAX_COPIED + 5 {
                tracing::warn!(target: "ekos_semantic::x", "dangling edge");
            }
        });
        let copied = events
            .iter()
            .filter(|e| e["msg"] == "dangling edge")
            .count();
        assert_eq!(copied as u64, MAX_COPIED);
        let rest = events
            .iter()
            .find(|e| e["msg"] == "5 more warning(s) not copied")
            .unwrap();
        assert_eq!(rest["by_target"]["ekos_semantic::x"], 5);
        assert_eq!(events.last().unwrap()["warnings"], MAX_COPIED + 5);
    }

    #[test]
    fn counts_may_not_overwrite_the_lines_own_keys() {
        assert_eq!(counts_of(Some(r#"{"cmd":"x","rows":3}"#)), r#""rows":3"#);
        assert_eq!(counts_of(Some("7")), r#""data":7"#);
        assert_eq!(counts_of(Some("null")), "");
        assert_eq!(counts_of(None), "");
    }

    #[test]
    fn only_pipeline_commands_get_a_log_and_the_environment_names_the_file() {
        let config = EkosConfig::default();
        let cwd = Path::new("/w");
        assert!(for_command("ask", &config, cwd).is_none());
        let set = path_for(&config, cwd, "build", Some(PathBuf::from("/runs/42.jsonl")));
        assert_eq!(set, PathBuf::from("/runs/42.jsonl"));
        let default = path_for(&config, cwd, "build", None);
        assert!(default.starts_with(config.ekos_dir(cwd).join("runs")));
        assert!(default.to_string_lossy().ends_with("-build.jsonl"));
    }

    #[test]
    fn a_command_that_changes_something_gets_a_log_named_by_its_path() {
        let config = EkosConfig::default();
        let dir = tempfile::tempdir().unwrap();
        let path: Vec<String> = ["lakehouse", "marts", "build"].map(String::from).into();
        for class in [None, Some(Class::Read), Some(Class::Local)] {
            assert!(for_class(&path, class, &config, dir.path()).is_none());
        }
        assert!(for_class(&[], Some(Class::Write), &config, dir.path()).is_none());
        if std::env::var_os("EKOS_RUN_LOG").is_some() {
            return; // the environment names the file; covered above
        }
        let log = for_class(&path, Some(Class::External), &config, dir.path()).unwrap();
        log.finish(&Ok(()));
        let runs = config.ekos_dir(dir.path()).join("runs");
        let file = std::fs::read_dir(&runs)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(
            file.to_string_lossy()
                .ends_with("-lakehouse-marts-build.jsonl")
        );
        let text = std::fs::read_to_string(file).unwrap();
        let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["cmd"], "lakehouse marts build");
        assert_eq!(first["msg"], "started");
        assert!(text.contains(r#""stage":"llm""#));
    }
}
