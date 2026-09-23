# Devlog 201 — v1.0.0 and v1.0.1: the first release, and what shipping it taught

**Date:** 2026-09-23
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Cut the project's first tagged release as **1.0.0** rather than the 0.1.0 that devlog_200's
release pipeline was written for. The workspace had carried `version = "0.1.0"` since Phase 0 and
nothing had ever revisited it. Bumping to 1.0.0 is not a formality: a version number is a promise,
and `CHANGELOG.md` now states the exact one — which surfaces are stable, which are explicitly not,
and what a breaking change to each would cost.

---

## Why 1.0.0 and not 0.1.0

The README has carried a versioning roadmap since the beginning, and its last row is
`v1.0 — Enterprise Knowledge Compiler`. Every row above it is shipped: compiler infrastructure,
observation layer, knowledge recovery, identity resolution, the ledger, the runtime. Releasing the
finished roadmap as `0.1.0` would have understated it by an order of magnitude, and `0.x` carries
a specific meaning in the ecosystem — *nothing here is stable yet* — which is false for a system
with 153 RFCs, a documented append-only ledger format and an MCP surface that several editors are
already configured against.

The cost of the number is real and worth naming: under semver, breaking any covered surface after
this means 2.0.0. That is the point. The constraint is the feature.

## What the promise actually covers

The judgement call was scope, not the number. Committing to everything would be dishonest — the
Rust crate APIs genuinely are still moving as connectors land. Committing to nothing would make
the number meaningless. `CHANGELOG.md` draws the line explicitly:

| Covered (breaking → 2.0.0) | Not covered |
|---|---|
| Pipeline verbs and their order, and existing flags | Rust crate APIs (unpublished; the SDK traits still move) |
| MCP tool names and argument shapes | Human-readable report text and formatting |
| On-disk ledger format (1.x reads 1.x; SQLite workspaces keep serving) | LLM-generated prose — only the grounding/citation contract is stable |
| The four semantic primitives and the evidence rule | Anything behind an opt-in experimental flag |
| `ekos.toml` schema — additive, defaults preserve behaviour | |

The RFC 0149 extension seam **is** covered, even though the crate APIs are not — out-of-tree
builds depend on it and it is the whole point of the open-core split.

`--json` surfaces already carry a `schema_version`, which is what makes "report text is not
stable" a safe thing to say: machines have a stable form to read, humans get prose that can
improve.

## Known limitations shipped as known

The release notes keep a real limitations section rather than a feature list only: whole-file
all-or-nothing SQL DDL parsing, exact-match-only identity merging, five scaffolded connectors
never exercised against live accounts, no cross-file call-chain tracing, no release signing, no
crates.io. A 1.0.0 that hides these would be worth less than a 0.1.0 that names them — and
`ekos coverage` (devlog_200) now reports the SQL one at runtime instead of letting it stay silent.

## What changed

Mechanically small: `[workspace.package] version`, which every one of the 34 crates inherits, plus
the version references in the README, RFC 0153, the release workflow's own example command, and
the TODO item. `benchmark/` and `tests/integration/` stay at `0.1.0` deliberately — they are
separate workspaces holding internal test and benchmark packages, not released artifacts, and
bumping them would imply a stability promise nobody is making about them.

`ekos --version` reports `ekos 1.0.0`, which is RFC 0153's acceptance criterion 10.

---

## Knowledge Captured

- **A version number is a scope decision, not a formality.** The useful question was never "is
  this 1.0-quality?" but "which surfaces am I willing to be held to?". Writing that table was most
  of the work, and it is the part that will matter in a year.
- **`0.x` says something specific and it was false here.** It reads as "expect breakage", which
  actively misrepresents a project whose ledger format and MCP surface people already depend on.
  Leaving a Phase-0 placeholder in place for 200 devlogs was the actual mistake.
- **Stability promises need an escape hatch that is itself stable.** Saying "the Rust APIs are not
  covered" is only safe because the RFC 0149 extension seam *is* — otherwise out-of-tree builds
  would have nothing to rely on. Same shape as `--json`'s `schema_version` making "report text is
  not stable" safe to say.
- **The tag is not the release.** `git tag` locally changes nothing public; the push is what
  triggers `.github/workflows/release.yml`, builds the six targets and creates the GitHub Release
  that `install.sh` resolves. Until then the README's install command has nothing to find.
- **A skip-CI marker on a release commit silently cancels the release.** This project commits to
  `main` locally with the marker as a matter of routine, and the first v1.0.0 commit inherited it.
  GitHub applies that marker to **tag** pushes as well — it reads the head commit of the push, and
  a tag push's head commit is the tagged commit — so `release.yml` would never have run, no
  binaries would have been built, and `install.sh` would have kept resolving nothing. Caught
  before pushing only by re-reading the commit that the tag pointed at. The release commit now
  deliberately carries no marker.
- **The marker is matched literally, anywhere in the message.** The first fix added a paragraph
  *explaining* why the marker was omitted — and wrote it out, which re-armed the exact behaviour
  it was documenting. A commit message cannot quote it; it has to describe it.
- **A `targets:` input and a `rust-toolchain.toml` are two different toolchains.** The action
  installs into the one it selects; cargo uses the one the file pins. Nothing warns about the
  mismatch — the target is genuinely installed, just not where it is needed, and the error that
  surfaces (`can't find crate for 'core'`) names neither toolchain.
- **Only non-host rows can expose a cross-compilation defect**, so a green matrix that happens to
  be all-native proves nothing about the cross ones. Here four of six rows were their runner's
  native triple and passed; both failures were the two that were not.
- **cc-rs's tool name for musl is not the name Ubuntu ships.** It wants
  `x86_64-linux-musl-gcc`; `musl-tools` provides `musl-gcc`. A workspace with no C dependencies
  would never hit this — this one compiles bundled SQLite and zstd, so it does.
- **Fixing one cross-compilation stage reveals the next.** The std fix alone would have produced
  a second failed run with a completely different-looking error. Worth walking the whole chain
  locally before re-triggering a public release, rather than one fix per run.
- **Build release binaries on the OLDEST supported runner, never `ubuntu-latest`.** A glibc
  binary runs on its build-time glibc or newer. `ubuntu-latest` silently tracks the newest image,
  so it quietly raises the floor under every user each time GitHub moves it — the single most
  common "your binary doesn't run" report for Rust CLIs, reproduced here on the first try.
- **A CI job that verifies an install on the image that built it verifies nothing.** The `verify
  install.sh` job passed on the same `ubuntu-latest` that produced the binary, so the one glibc
  version certain to work was the only one tested. The defect was found by a human running the
  public one-liner on a different machine. Pinning the build runner narrows this, but the real
  lesson is that a verification step has to differ from the build environment in the dimension it
  claims to check.
- **Prefer an empirical check over a version heuristic.** `install.sh` could have parsed
  `ldd --version` and compared numbers. Running `--version` on the binary it just verified is
  shorter, has no parsing to get wrong, and catches reasons to fail that nobody enumerated.
- **A green release pipeline is not a working release.** Six assets, all jobs green, and the
  primary Linux artifact did not start. Nothing short of installing the published thing on a
  machine that did not build it would have shown that.

---

## The first real release run failed two of six targets

Pushing the tag is what finally executed `.github/workflows/release.yml`, and it surfaced two
defects that nothing local could have caught. Both are the same shape: a cross-compilation
assumption that is invisible when you only ever build for the host.

### 1. The pinned toolchain had no std for the cross targets

`dtolnay/rust-toolchain@stable` with a `targets:` input installs that target into the toolchain
**the action selects** — stable. Every `cargo` command in the job runs under the toolchain
`rust-toolchain.toml` pins — 1.98.0. So the cross targets had std installed for a toolchain
nothing used, and the build died on the first crate:

```
error[E0463]: can't find crate for `core`
error: could not compile `cfg-if` (lib) due to 1 previous error
```

Host-native rows were unaffected — their std ships with the toolchain — which is exactly why
**four of six passed and only `x86_64-unknown-linux-musl` and `x86_64-apple-darwin` failed**: the
two rows that are not their runner's native triple. The fix is to add the target in the working
directory, where the toolchain file applies, rather than through the action:

```yaml
- run: |
    rustup show active-toolchain
    rustup target add ${{ matrix.target }}
```

Verified locally before changing the workflow: `rustup target add x86_64-unknown-linux-musl` run
inside the repository installs into `1.98.0-x86_64-unknown-linux-gnu (overridden by
'…/rust-toolchain.toml')`, not into stable.

### 2. `musl-tools` does not provide the binary cc-rs looks for

Fixing the std problem locally moved the failure one stage later, to the C dependencies (bundled
SQLite and zstd are real C):

```
error occurred in cc-rs: failed to find tool "x86_64-linux-musl-gcc":
No such file or directory (os error 2)
```

cc-rs looks for `x86_64-linux-musl-gcc`. Ubuntu's `musl-tools` installs `musl-gcc` and nothing
under the other name. So the musl row would have failed **again** on the next run, one stage
further along, for an unrelated-looking reason. `CC_x86_64_unknown_linux_musl: musl-gcc` is the
lever; confirmed locally by setting that variable to a compiler that does exist and watching the
same `cargo check` go from `ToolNotFound` to `Finished`.

**Honest limit of local verification:** without `musl-gcc` on this machine (it needs root to
install), the two *mechanisms* were verified locally but the full static musl link was not. CI is
the only thing that can confirm that one.

---

## v1.0.1 — the published binary did not run on the machine that built it

v1.0.0 published six assets and the workflow went green, including the `verify install.sh` job.
Then installing it here, through the public one-liner, exactly as a reader would:

```
/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

The `x86_64-unknown-linux-gnu` asset was built on `ubuntu-latest` — 24.04, glibc 2.39. A glibc
binary runs on that version **or newer**, never older, so building the release on the newest
available image is exactly backwards. This machine runs glibc 2.35, and so do Ubuntu 22.04,
Debian 12 and RHEL 9. The default Linux asset — the one `install.sh` picks for the most common
platform there is — could not start on a large share of real machines.

Worse, **CI could not have caught it.** The `verify` job installs on `ubuntu-latest`, the same
image that produced the binary, so the one glibc version guaranteed to work is the one being
tested. A green verify job proved nothing about the property that actually mattered.

The musl asset, checked on the same machine, is `static-pie linked` and runs fine. Two fixes,
because either alone leaves a gap:

1. **Build glibc targets on the oldest supported runner**, `ubuntu-22.04` / `ubuntu-22.04-arm`
   (glibc 2.35), not `ubuntu-latest`. This lowers the floor for everyone without musl's
   allocator cost.
2. **`install.sh` runs the binary before installing it**, and on x86_64 Linux falls back to the
   static musl build when it will not start. Checking empirically beats parsing `ldd --version`:
   it covers every reason a build might not start on a machine, not only the glibc one we now
   know about.

The fallback was verified against the **real, published v1.0.0 release** on this machine — which
genuinely cannot run that release's gnu binary, making it an honest test rig rather than a mock:

```
Downloading ekos-1.0.0-x86_64-unknown-linux-gnu.tar.gz (v1.0.0)...
Checksum verified.

The x86_64-unknown-linux-gnu build does not run on this system (most likely an older glibc).
Falling back to the fully static x86_64-unknown-linux-musl build.

Downloading ekos-1.0.0-x86_64-unknown-linux-musl.tar.gz (v1.0.0)...
Checksum verified.
Installed ekos v1.0.0
```

Shipped as **v1.0.1**. v1.0.0 is left published rather than deleted: `install.sh` resolves
`latest`, so every new install gets the fix, and retracting a release someone may already hold is
worse practice than a fast patch.

Remaining gap, stated in `CHANGELOG.md` rather than left implicit: there is no musl build for
aarch64 Linux, so that platform has no fallback. Adding the row was deliberately not done in the
same change — a speculative matrix row that fails takes the whole `release` job with it, because
`release` needs every `build`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/Cargo.toml` | `[workspace.package] version` 0.1.0 → 1.0.0 (inherited by all 34 crates) |
| `ekos/Cargo.lock` | regenerated for the bump |
| `CHANGELOG.md` | rewritten for v1.0.0, with the explicit stability-promise table and a real known-limitations section |
| `README.md` | versioning-roadmap note now says 1.0.0, released, and points at the changelog |
| `TODO.md` | tag item ticked; the remaining step is the push |
| `ekos/docs/rfcs/0153-…md` | v0.1.0 → v1.0.0 throughout |
| `.github/workflows/release.yml` | example tag command; the two cross-compilation fixes; then glibc runners pinned to 22.04 |
| `install.sh` | runs the binary before installing, with a static-musl fallback on x86_64 Linux |
| `CHANGELOG.md` | v1.0.1 entry; aarch64 fallback gap named in known limitations |
| `devlogs/devlog_200.md` | changelog row notes the renumbering |

**Not a file change, but the most important thing in this devlog:** the release commit carries no
skip-CI marker, and must not gain one. See Knowledge Captured.
