//! `ekos marketing publish` (RFC 0030).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use ekos_compiler_core::EkosConfig;
use ekos_marketing::{
    Importance, NoopPublisher, PostedStore, Publisher, TwitterPublisher, generate_tweet,
    importance::classify,
};
use ekos_recovery::{AnthropicProvider, CachedLlmProvider, LlmProvider, OllamaProvider};

pub async fn publish(
    config: &EkosConfig,
    cwd: &Path,
    devlog_arg: Option<String>,
    yes: bool,
    dry_run: bool,
) -> Result<()> {
    let marketing_dir = cwd.join("marketing");

    // Load marketing/.env if present — never overrides a variable already set
    // in the real environment (dotenvy's default), so `export`ing directly
    // still takes precedence. Silently absent is fine: TWITTER_*/ANTHROPIC_*
    // may already be set in the shell instead, and dry-run mode needs none
    // of this at all.
    dotenvy::from_path(marketing_dir.join(".env")).ok();

    let devlog_path = resolve_devlog_path(cwd, devlog_arg.as_deref()).ok_or_else(|| {
        anyhow!("no devlog found (pass a path, a number, or ensure devlog_*.md files exist)")
    })?;

    let filename = devlog_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string);
    let content = std::fs::read_to_string(&devlog_path)
        .with_context(|| format!("cannot read {}", devlog_path.display()))?;
    let devlog = ekos_marketing::devlog::parse(&content, filename.as_deref())
        .with_context(|| format!("cannot parse {}", devlog_path.display()))?;

    log_line(&marketing_dir, &format!("Read {}", devlog_path.display()));

    let importance = classify(&devlog);
    println!(
        "Devlog {}: \"{}\" — importance {:?}",
        devlog.number, devlog.title, importance
    );
    log_line(&marketing_dir, &format!("Importance {importance:?}"));
    if importance == Importance::Low {
        println!("Importance LOW (docs/tests/refactor only) — no tweet.");
        return Ok(());
    }

    let mut store = PostedStore::load(&marketing_dir).with_context(|| {
        format!(
            "cannot load {}",
            marketing_dir.join("posted/tweets.json").display()
        )
    })?;
    if let Some(existing) = store.is_posted(devlog.number) {
        println!(
            "Already posted for devlog {}: tweet {} on {}. Skipping.",
            devlog.number, existing.tweet_id, existing.date
        );
        log_line(
            &marketing_dir,
            &format!("Duplicate post for devlog {} — skip", devlog.number),
        );
        return Ok(());
    }

    let artifact_dir = config.artifact_dir(cwd);
    let llm = select_llm_provider(config, &artifact_dir)?;
    let draft = generate_tweet(&*llm, &config.marketing, &devlog)
        .await
        .context("tweet generation failed")?;
    log_line(&marketing_dir, "Tweet generated");

    let approved_text = match approve(&draft.text, yes)? {
        Some(text) => text,
        None => {
            println!("Aborted — tweet not published.");
            log_line(&marketing_dir, "Not approved — aborted");
            return Ok(());
        }
    };
    log_line(&marketing_dir, "Approved");

    // Whether this run actually reaches X, not just whether --dry-run was passed: the
    // config gate ([marketing.twitter] enabled/dry-run) can also force a no-op publish, and
    // the dedup record below must track the real outcome, not the CLI flag alone — otherwise
    // a config-gated no-op run gets recorded as posted and blocks every real run after it.
    let publishes_for_real =
        !dry_run && config.marketing.twitter.enabled && !config.marketing.twitter.dry_run;
    let publisher: Box<dyn Publisher> = if publishes_for_real {
        Box::new(TwitterPublisher::from_env().map_err(|e| anyhow!("{e} — cannot publish to X"))?)
    } else {
        Box::new(NoopPublisher)
    };

    let tweet_id = publisher
        .publish(&approved_text)
        .await
        .context("publish failed")?;
    println!("Published. Tweet id: {tweet_id}");
    log_line(&marketing_dir, &format!("Published. Tweet ID {tweet_id}"));

    if publishes_for_real {
        store.record(
            devlog.number,
            tweet_id,
            Utc::now().format("%Y-%m-%d").to_string(),
            devlog.title.clone(),
        );
        store
            .save()
            .context("cannot save marketing/posted/tweets.json")?;
    }

    Ok(())
}

/// Selects an LLM provider the same way `ekos recover` does (RFC 0021's `[llm] provider =
/// "ollama"` routing, else Anthropic), but — unlike `recover`, which has a legitimate
/// "structural analysis only" degraded mode to fall back to — errors out clearly when no
/// provider is usable instead of silently falling back to a mock. There is no valid degraded
/// mode for drafting a tweet: a mock response would just surface later as a confusing
/// "missing field `tweet`" JSON error instead of a clear "no API key" one.
fn select_llm_provider(config: &EkosConfig, artifact_dir: &Path) -> Result<Arc<dyn LlmProvider>> {
    let cache_dir = artifact_dir
        .parent()
        .unwrap_or(artifact_dir)
        .join("llm-cache");
    std::fs::create_dir_all(&cache_dir).ok();

    if config.llm.provider.as_deref() == Some("ollama") {
        return Ok(Arc::new(CachedLlmProvider::new(
            OllamaProvider::from_env_with_model(config.llm.model.as_deref()),
            cache_dir,
        )));
    }

    let key_env = config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or("ANTHROPIC_API_KEY");
    let provider = AnthropicProvider::from_env_var(key_env).map_err(|_| {
        anyhow!(
            "{key_env} not set and no [llm] provider = \"ollama\" configured in ekos.toml — \
             an LLM is required to draft a tweet"
        )
    })?;
    Ok(Arc::new(CachedLlmProvider::new(provider, cache_dir)))
}

/// `DEVLOG` may be a path, a bare number ("28"), the literal "latest", or omitted (= latest).
/// Bare numbers and "latest"/omitted resolve against `cwd/devlogs/` (devlogs moved out of the
/// repo root); an explicit path argument is still resolved as given (absolute, or relative to
/// `cwd`), so `ekos marketing publish devlogs/devlog_28.md` keeps working unchanged.
fn resolve_devlog_path(cwd: &Path, devlog_arg: Option<&str>) -> Option<PathBuf> {
    let devlogs_dir = cwd.join("devlogs");
    match devlog_arg {
        None | Some("latest") => ekos_marketing::devlog::find_latest(&devlogs_dir),
        Some(arg) => {
            if let Ok(n) = arg.parse::<u32>() {
                let path = devlogs_dir.join(format!("devlog_{n}.md"));
                return path.exists().then_some(path);
            }
            let path = Path::new(arg);
            let candidate = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            candidate.exists().then_some(candidate)
        }
    }
}

/// Interactive Y/N/E approval, per RFC 0030 ("No automatic publishing."). `--yes` (`auto`)
/// skips straight to approval-as-drafted. Returns `None` on reject/EOF.
fn approve(draft: &str, auto: bool) -> Result<Option<String>> {
    if auto {
        return Ok(Some(draft.to_string()));
    }

    let mut text = draft.to_string();
    let stdin = std::io::stdin();
    loop {
        println!(
            "\nTweet Preview\n---------------------------------\n{text}\n---------------------------------"
        );
        print!("Approve? [Y]es / [N]o / [E]dit: ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(None); // EOF (non-interactive/piped invocation with no answer) — abort.
        }
        match line.trim().to_lowercase().as_str() {
            "y" | "yes" | "" => return Ok(Some(text)),
            "n" | "no" => return Ok(None),
            "e" | "edit" => {
                print!("New tweet text: ");
                std::io::stdout().flush().ok();
                let mut edited = String::new();
                if stdin.lock().read_line(&mut edited)? == 0 {
                    return Ok(None);
                }
                text = edited.trim().to_string();
            }
            other => println!("Unrecognized answer '{other}' — enter Y, N, or E."),
        }
    }
}

fn log_line(marketing_dir: &Path, message: &str) {
    let logs_dir = marketing_dir.join("logs");
    if std::fs::create_dir_all(&logs_dir).is_err() {
        return;
    }
    let line = format!("{} {message}\n", Utc::now().to_rfc3339());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("marketing.log"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Proves `marketing/.env` actually gets loaded — `dotenvy::from_path` is
    /// a two-line call in `publish()` with nothing else exercising it, so a
    /// direct test is worth more here than trusting "well-known crate, must
    /// be fine": confirms the file we tell users to create really does reach
    /// `std::env::var` the way `TwitterPublisher::from_env()`/`select_llm_provider`
    /// expect. Uses a unique var name to avoid colliding with any other test
    /// or real environment state (env vars are process-global).
    #[test]
    fn dotenv_file_populates_the_process_environment() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "EKOS_TEST_DOTENV_MARKER_1=loaded-from-file\n",
        )
        .unwrap();

        // SAFETY: test-only, a uniquely-named var no other test touches —
        // the documented hazard is concurrent env mutation racing another
        // thread's read; nothing else in this process reads this name.
        unsafe {
            std::env::remove_var("EKOS_TEST_DOTENV_MARKER_1");
        }
        dotenvy::from_path(dir.path().join(".env")).ok();
        assert_eq!(
            std::env::var("EKOS_TEST_DOTENV_MARKER_1").as_deref(),
            Ok("loaded-from-file")
        );
        unsafe {
            std::env::remove_var("EKOS_TEST_DOTENV_MARKER_1");
        }
    }

    /// The safety property the code comment in `publish()` promises: a
    /// variable already exported in the real environment must win over
    /// whatever `marketing/.env` says — so a user who exports fresh,
    /// rotated credentials directly in their shell is never silently
    /// overridden by a stale value left in the file.
    #[test]
    fn dotenv_file_never_overrides_an_already_set_var() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "EKOS_TEST_DOTENV_MARKER_2=from-file\n",
        )
        .unwrap();

        // SAFETY: test-only, a uniquely-named var no other test touches —
        // see the comment in the test above.
        unsafe {
            std::env::set_var("EKOS_TEST_DOTENV_MARKER_2", "from-real-shell");
        }
        dotenvy::from_path(dir.path().join(".env")).ok();
        assert_eq!(
            std::env::var("EKOS_TEST_DOTENV_MARKER_2").as_deref(),
            Ok("from-real-shell"),
            "an already-exported var must win over marketing/.env"
        );
        unsafe {
            std::env::remove_var("EKOS_TEST_DOTENV_MARKER_2");
        }
    }

    /// A missing `marketing/.env` must never fail the command — most users
    /// will export real env vars directly and never create the file at all.
    #[test]
    fn missing_dotenv_file_is_a_silent_no_op() {
        let dir = tempdir().unwrap();
        // No .env written — from_path on a nonexistent file must not panic.
        let result = dotenvy::from_path(dir.path().join(".env"));
        assert!(result.is_err(), "nonexistent file returns Err, not panic");
    }

    #[test]
    fn resolve_devlog_path_none_finds_latest() {
        let dir = tempdir().unwrap();
        let devlogs = dir.path().join("devlogs");
        std::fs::create_dir(&devlogs).unwrap();
        std::fs::write(devlogs.join("devlog_5.md"), "x").unwrap();
        std::fs::write(devlogs.join("devlog_9.md"), "x").unwrap();
        let resolved = resolve_devlog_path(dir.path(), None).unwrap();
        assert_eq!(resolved.file_name().unwrap(), "devlog_9.md");
    }

    #[test]
    fn resolve_devlog_path_accepts_bare_number() {
        let dir = tempdir().unwrap();
        let devlogs = dir.path().join("devlogs");
        std::fs::create_dir(&devlogs).unwrap();
        std::fs::write(devlogs.join("devlog_28.md"), "x").unwrap();
        let resolved = resolve_devlog_path(dir.path(), Some("28")).unwrap();
        assert_eq!(resolved.file_name().unwrap(), "devlog_28.md");
    }

    #[test]
    fn resolve_devlog_path_bare_number_missing_file_is_none() {
        let dir = tempdir().unwrap();
        assert!(resolve_devlog_path(dir.path(), Some("999")).is_none());
    }

    #[test]
    fn resolve_devlog_path_accepts_explicit_relative_path() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("custom_name.md"), "x").unwrap();
        let resolved = resolve_devlog_path(dir.path(), Some("custom_name.md")).unwrap();
        assert_eq!(resolved.file_name().unwrap(), "custom_name.md");
    }

    #[test]
    fn approve_with_auto_true_skips_prompt() {
        let result = approve("draft text", true).unwrap();
        assert_eq!(result, Some("draft text".to_string()));
    }
}
