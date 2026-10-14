# Devlog 267 — Offline install package and install instructions

**Date:** 2026-10-14
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

The only way to get a prebuilt `ekos` was a GitHub release, and the newest one (v1.0.3) is 78
commits behind `main`: none of RFC 0178–0183 is in it. `scripts/package-release.sh` now builds an
install package for Linux x86_64 from any commit, in the same shape a release has, so the
existing `install.sh` installs it offline. Install instructions were added to the package
(`INSTALL.md`), the README, the deployment guide and the capabilities page. Package built:
`dist/ekos-1.0.3-dev.35adb7b-release.tar.gz` (42 MB).

---

## Install package

### What was built

| Piece | What it does |
|---|---|
| `scripts/package-release.sh` | `git archive HEAD` → glibc build on the host, static musl build in `rust:<pinned>-slim-bookworm`, each packed as `release.yml` packs it; one `SHA256SUMS`; `install.sh`; `INSTALL.md` from a template; installs the result into a throwaway home and runs `ekos --version` before it writes the final tarball |
| `deploy/package/INSTALL.md` | Template (`@VERSION@`, `@COMMIT@`, `@CARGO_VERSION@`): contents, install with the installer and by hand, other locations, first run in place and as a home project, connecting an agent, upgrade and removal, troubleshooting |
| README "Offline, from an install package" | How to build one and install it |
| `docs/deployment.md` "A server without internet access" | The console image by `docker save` / `docker load`; the package for people who want the CLI |
| Capabilities page | An "Offline install package" card in Install & first run |

### Implementation details worth remembering

- Version label: the tag when HEAD is tagged, otherwise `<crate version>-dev.<short commit>`. The
  binary still says `ekos 1.0.3` (the crate version); the package name carries the commit.
- `install.sh` needed no change: `EKOS_BASE_URL=file://$PWD` and `EKOS_VERSION=v<version>` were
  already enough (that is how `scripts/test-install.sh` drives it).
- The musl container runs as root (apt, rustup) with its **own** registry volume in the cache,
  then chowns its output back. Mounting the host's `~/.cargo/registry` into a root container can
  leave root-owned files there.

### Verified

- Both builds from the committed tree; gnu needs `GLIBC_2.34` at most; musl is `static-pie` and
  runs in `alpine:latest`.
- `INSTALL.md` step 1 run verbatim from the single tarball with a clean environment: checksum
  verified, installed to `~/.ekos/bin`, PATH hint printed. The by-hand checksum line prints `OK`.
- The first-run walkthrough (`project add` with two named sources, the five stages, `query find`,
  `home backup`) on the built binary: no model configured, results from both sources, backup in
  `<home>/backups/`, sources unchanged.

### Decisions

- **From the committed tree only**, never the working tree: a package says which commit it is,
  and that must be true. Uncommitted edits are reported and left out.
- **Linux x86_64 only.** macOS and aarch64 need their own machines (or cross toolchains this
  machine lacks); the GitHub release matrix still covers them on a tag.
- **The image is not in the package.** It is about 570 MB unpacked against the package's 42 MB, and only a server needs it;
  `docker save` covers that case and is documented.

---

## Knowledge Captured

- **`rustup target add` outside the source directory adds the target to the wrong toolchain.**
  In `rust:1.98-slim-bookworm` the first musl build failed with `can't find crate for core`
  (E0463): the target went to the image's default toolchain while `rust-toolchain.toml` selected
  another. Run it after `cd` into the checkout. `release.yml` documents the same trap.
- **`CARGO_BUILD_JOBS` does not reach a `docker run` unless passed** (`-e CARGO_BUILD_JOBS`).
  On this 16 GB machine an unlimited release build next to other work risks the OOM killer.
- A seeded cache works across source paths for dependencies only: moving an earlier target
  directory into `~/.cache/ekos-package` cut each build to about 3 minutes.

---

## Files Changed

| File | Change summary |
|---|---|
| `scripts/package-release.sh` | New: builds, packs, test-installs the offline package |
| `deploy/package/INSTALL.md` | New: the package's install guide (template) |
| `README.md` | "Offline, from an install package"; upgrade line mentions packages |
| `docs/deployment.md` | Offline image install; package pointer; table row |
| `docs/generated/ekos-self-documentation.html` | "Offline install package" card |
| `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Entry, tick + remainder, command and devlog pointer |
| `.gitignore` | `/dist/` |
