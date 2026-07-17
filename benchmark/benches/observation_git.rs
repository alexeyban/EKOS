use criterion::{Criterion, criterion_group, criterion_main};
use ekos_observation_sdk::{Observer, ScanContext};
use ekos_plugin_git::GitObserver;
use std::process::Command;

/// Build a small throwaway git repo with a handful of commits to scan.
fn fixture_repo(commits: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "bench@example.com"]);
    run(&["config", "user.name", "bench"]);

    for i in 0..commits {
        std::fs::write(dir.path().join("file.txt"), format!("commit {i}")).unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", &format!("commit {i}")]);
    }

    dir
}

fn bench_observation_git(c: &mut Criterion) {
    let dir = fixture_repo(20);
    let observer = GitObserver::new();
    let ctx = ScanContext::new(dir.path());
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("observation_git_scan_20_commits", |b| {
        b.to_async(&rt)
            .iter(|| async { observer.scan(&ctx).await.unwrap() });
    });
}

criterion_group!(benches, bench_observation_git);
criterion_main!(benches);
