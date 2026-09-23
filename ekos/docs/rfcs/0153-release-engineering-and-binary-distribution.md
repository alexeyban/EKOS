# RFC 0153 — Release Engineering and Binary Distribution

**Status:** Accepted
**Date:** 2026-09-23
**Depends on:** RFC 0113 (the `distributed` cargo feature this must *not* build by default),
RFC 0149 (extension seam — out-of-tree builds consume the same released artifacts),
RFC 0152 (first-run self-verification — what a newly-installed binary must do well)

---

## 1. Motivation

EKOS has 199 devlogs, 151 RFCs, 34 crates, 12 connectors, a distributed storage tier and 32
published presentation decks. It has **zero GitHub releases**, is not on crates.io, and its
`## Installation` section — 130 lines into a 1,380-line README — says:

```
git clone …
cd ekos
cargo install --path crates/cli
```

That instruction asks a reader to install a Rust toolchain and compile 34 crates, including
`tantivy`, `oxc`, `syn`, a bundled SQLite and a rustls stack, before they can see anything. Every
one of the 32 decks, the token benchmark, and every future post terminates at that wall. The
project's distribution surface is currently: *be willing to compile a Rust workspace.*

This is not a marketing problem to be solved with more content. It is a missing artifact. The
single highest-leverage change available is that a reader can run one command and have a working
`ekos` binary.

Two supporting facts make this cheap:

- **Nothing in the dependency graph resists static linking.** `rusqlite` is `bundled` (compiles
  SQLite from source, no system library), `reqwest` is `rustls-tls` with `default-features =
  false` (no OpenSSL), and `tantivy`/`oxc`/`syn` are pure Rust. There are no `*-sys` or `bindgen`
  dependencies in the workspace. The one native tool, `tesseract`, is *shelled out to* and
  degrades to `OcrError::Unavailable` when absent — it is never linked.
- The `distributed` feature is already off by default, so a stock build never pulls the
  `object_store` stack.

The one real gap is reproducibility: there is no `rust-toolchain.toml` and no `rust-version`, so
"reproducible builds" (a stated coding rule) is currently enforced by nothing.

## 2. Goals / Non-goals

**Goals**

- A tagged `v0.1.0` GitHub Release carrying prebuilt, checksummed `ekos` binaries for the
  platforms a reader of this project plausibly has.
- A one-line install that works without a Rust toolchain.
- A pinned toolchain, so a release built today and rebuilt in six months is the same compiler.
- Release metadata (`repository`, `description`, `rust-version`, `readme`, `keywords`) on the
  workspace package — required for crates.io later, and correct to have regardless.
- A README `## Installation` that leads with the binary and keeps the source build as the second
  option, not the only one.

**Non-goals**

- **Publishing to crates.io in this RFC.** `cargo install ekos` requires all 30 internal path
  dependencies to be published, versioned and ordered — a separate project with its own failure
  modes (a botched publish is irreversible; crates.io yanks do not delete). §6 records the
  ordering and the blockers so it can be done deliberately, not as a side effect of a release
  workflow. The name `ekos` is confirmed unclaimed on crates.io as of 2026-09-23.
- Homebrew tap, Nix flake, Debian/RPM packaging, Docker image for the CLI, `winget`/Chocolatey.
  All are downstream of having a release to package; none should gate the first one.
- Signing (Sigstore/cosign) or SLSA provenance. Correct eventually; a SHA256SUMS file is the
  honest v0.1.0 posture and the RFC says so rather than implying more.
- Auto-publishing on every merge to `main`. Releases are tag-triggered and deliberate.

## 3. Targets

| Target | Runner | Rationale |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` | The default Linux developer machine. |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` + `musl-tools` | Fully static — runs on any glibc vintage, in Alpine, and in slim containers. The artifact that makes "works everywhere" true rather than aspirational. |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | Native ARM runner; no cross-toolchain needed. ARM servers and ARM CI are now the common case, not the exception. |
| `aarch64-apple-darwin` | `macos-14` | Every Apple Silicon Mac. |
| `x86_64-apple-darwin` | `macos-15` (cross via target) | Intel Macs, still widely in use. |
| `x86_64-pc-windows-msvc` | `windows-latest` | The README already documents a Windows source install, so the platform is claimed; a release must either honour it or stop claiming it. |

All built with `--locked`, default features only, and `-p ekos` — the CLI binary, not the whole
workspace.

## 4. Build profile

The workspace has no `[profile.release]` section, so releases would ship default-profile
binaries. This adds one:

```toml
[profile.release]
strip = "symbols"   # the single largest size win for a workspace this size
lto = "thin"        # cross-crate inlining across 34 crates; "fat" costs CI minutes for little gain
codegen-units = 16  # left at the default — "1" trades real build time for marginal runtime gain
panic = "unwind"    # NOT "abort": `mcp serve` and the cluster/query-worker RPC layers rely on a
                    # panicking request thread not taking the whole process down with it
```

`panic = "abort"` is the tempting extra size win and is explicitly rejected: RFC 0115's TCP
transport spawns one OS thread per connection and RFC 0113's workers do the same, and a panic in
one connection must not kill a server serving others.

## 5. Release pipeline

`.github/workflows/release.yml`, triggered on `push` of a `v*` tag and by `workflow_dispatch`:

1. **`build` (matrix over §3)** — checkout, install the pinned toolchain with the target added,
   `cargo build --release --locked -p ekos --target <target>`, then package:
   `ekos-<version>-<target>.tar.gz` (`.zip` on Windows) containing the binary, `README.md` and
   `LICENSE`. Emit a per-target `.sha256`.
2. **`release`** — download every artifact, concatenate the per-target sums into one
   `SHA256SUMS`, create the GitHub Release with `gh release create`, attach everything.
3. The release body is hand-written for `v0.1.0` (`CHANGELOG.md`) — not generated from devlogs.
   199 devlogs are an engineering record written for this project's own memory; a release note is
   a different document with a different reader.

A `verify` job installs the freshly built Linux binary through `install.sh` and runs
`ekos --version` plus `ekos init --detect --dry-run` on a fixture workspace, so a release that
cannot actually be installed fails before it is published rather than after a user reports it.

### `install.sh`

Served from the repository (and therefore from the Pages site):

```
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
```

It detects OS/arch, resolves the latest release through the GitHub API (or honours
`EKOS_VERSION`), downloads the matching asset, **verifies it against `SHA256SUMS` before
unpacking**, and installs to `${EKOS_INSTALL_DIR:-$HOME/.local/bin}`. It prints the destination
and warns when that directory is not on `PATH`. It never uses `sudo`, never writes outside the
install directory, and exits non-zero with a real message on an unsupported platform rather than
installing something wrong.

Piping a script to a shell is a real supply-chain posture, not a neutral convenience. The script
is therefore short enough to read in full, the README shows the `curl … -o install.sh && less` form
alongside the one-liner, and the checksum step is not optional.

## 6. crates.io (deferred, documented)

For `cargo install ekos` to work, every internal crate must be published, because path
dependencies are not resolvable from the registry. The dependency order is the workspace member
order already encoded in `ekos/Cargo.toml`: `common` → `kir` → `artifact` → `compiler-core` →
`scheduler`/`compiler-sdk`/`observation-sdk`/`sql-dialect-sdk` → `segment-backend` → `cluster` →
`ledger` → `distributed` → `runtime` → `identity` → `recovery` → `ekl` → `semantic` → the
`plugins/*` → `docs-gen`/`dbt-gen`/`marketing`/`session`/`evals` → `cli`.

Blockers to resolve first, all of them real:

- Every crate needs `description` and `repository`; crates.io rejects a publish without them.
- Every internal dependency must carry a `version` alongside its `path`.
- Names: `ekos-common`, `ekos-kir` etc. must all be available, not just `ekos`.
- 30 publishes is 30 irreversible acts. It should be done once, deliberately, with a dry run
  (`cargo publish --dry-run`) per crate.

Recommended interim step: publish `ekos` v0.0.0 as a placeholder pointing at the repository, to
hold the name. This is mild name-holding and worth naming as such — the mitigating facts are that
it is the project's real name, the intent to publish is genuine, and the alternative is losing it
once the marketing push in RFC 0153's sibling work begins.

## 7. Alternatives considered

| Alternative | Verdict |
|---|---|
| `cargo-dist` | Rejected for v0.1.0. It generates a workflow very close to this one and adds a build-time dependency on a tool whose generated config must still be reviewed. A ~120-line workflow this project owns outright is easier to debug when a target breaks, and this repository's whole culture is explicit-over-magic. Reconsider once there are more than six targets. |
| Docker image as the primary distribution | Rejected as primary. `ekos` is a CLI that reads the user's own working tree and writes `.ekos/` in it; running it in a container means bind-mounting the workspace and fighting file ownership for every invocation. A container is right for the web console and for CI, and can come later. |
| GitHub Releases without musl | Rejected. glibc version skew is the single most common "your binary doesn't run" report for Rust CLIs, and musl costs one extra matrix row. |
| Build the whole workspace in release | Rejected. Only `-p ekos` ships; building 34 crates' release artifacts multiplies CI time for binaries nobody downloads. |
| `panic = "abort"` for a smaller binary | Rejected — see §4. |
| Generating release notes from devlogs | Rejected — see §5.3. |

## 8. Acceptance criteria

1. `rust-toolchain.toml` pins a stable version and is used by both CI and the release workflow.
2. `[workspace.package]` carries `description`, `repository`, `homepage`, `rust-version`,
   `keywords` and `categories`, the `ekos` package opts into them (workspace package metadata
   applies only where a member writes `<field>.workspace = true` — the CLI crate did not, so the
   first attempt set them and changed nothing), and `cargo metadata` reflects them.
   **`readme` is deliberately not among them:** its path resolves relative to each *member*
   crate, and a file outside a crate's own directory is not included by `cargo package`, so a
   workspace-level value would be wrong for every member. It belongs per-crate, as part of §6's
   deferred crates.io work.
3. `[profile.release]` exists with the §4 values, and `panic` is not `abort`.
4. `.github/workflows/release.yml` builds all six §3 targets on a `v*` tag.
5. Each artifact is a `.tar.gz`/`.zip` containing the binary, `README.md` and `LICENSE`, and a
   single `SHA256SUMS` covers every artifact in the release.
6. `install.sh` verifies the checksum before unpacking, installs without `sudo`, and exits
   non-zero with a readable message on an unsupported OS/arch. Verified against a locally served
   release (2026-09-23): a clean install succeeds; a tampered archive, an asset missing from
   `SHA256SUMS`, a 404, an unsupported OS and an unsupported architecture each refuse and install
   nothing. The asset name is regex-escaped before the `SHA256SUMS` lookup — unescaped, its `.`
   characters would match any byte, so a line naming a *different* file could satisfy the check.
7. `install.sh` passes `shellcheck` and is POSIX `sh`, not bash-only.
8. The `verify` job installs the built binary via `install.sh` and runs `ekos --version` and
   `ekos init --detect --dry-run` successfully.
9. `README.md`'s `## Installation` leads with the binary install; the source build remains,
   clearly labelled as the from-source path.
10. `ekos --version` reports the crate version (it must not print `0.0.0` or a git hash).
