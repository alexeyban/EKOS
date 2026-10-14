# Installing EKOS @VERSION@

This package installs the `ekos` command on a Linux x86_64 machine without downloading anything.
It was built from commit `@COMMIT@`.

## What is in the package

| File | What it is |
|---|---|
| `ekos-@VERSION@-x86_64-unknown-linux-gnu.tar.gz` | The binary, linked against glibc 2.34 or newer (Ubuntu 22.04, Debian 12, RHEL 9 and later). The faster of the two |
| `ekos-@VERSION@-x86_64-unknown-linux-musl.tar.gz` | The same binary, fully static. Runs on any Linux x86_64, including Alpine and older distributions |
| `SHA256SUMS` | The checksum of each archive |
| `install.sh` | The installer. It checks an archive against `SHA256SUMS` before unpacking it |
| `INSTALL.md` | This file |

Each archive holds one file that matters, `ekos`, plus the README and the licence. Nothing else
needs to be installed: no Rust, no Python, no database.

## 1. Install

Unpack the package and run the installer from inside it:

```bash
tar xzf ekos-@VERSION@-release.tar.gz
cd ekos-@VERSION@-release
EKOS_VERSION=v@VERSION@ EKOS_BASE_URL="file://$PWD" sh install.sh
```

The installer:

- verifies the archive against `SHA256SUMS` and stops if it does not match;
- runs the binary once, and takes the static build instead if the glibc one does not start;
- copies `ekos` to `~/.ekos/bin` and creates `~/.ekos/data`, where projects keep their
  configuration and compiled data;
- never uses `sudo` and writes nothing outside `~/.ekos`.

It prints a line to add to your shell profile when `~/.ekos/bin` is not on your `PATH`:

```bash
export PATH="$HOME/.ekos/bin:$PATH"
```

Then check it:

```bash
ekos --version        # ekos @CARGO_VERSION@
```

### Other places to install

| Setting | Effect |
|---|---|
| `EKOS_HOME=/srv/ekos` | The home is `/srv/ekos`: the binary goes to `/srv/ekos/bin`, projects to `/srv/ekos/data` |
| `EKOS_INSTALL_DIR=/usr/local/bin` | The binary goes there instead (needs write access; the installer does not use `sudo`) |

Set either in front of the install command. An earlier install in `~/.local/bin` is upgraded
where it is.

### Installing by hand

Without the installer, do what it does:

```bash
grep x86_64-unknown-linux-gnu SHA256SUMS | sha256sum -c -     # must print OK
tar xzf ekos-@VERSION@-x86_64-unknown-linux-gnu.tar.gz
install -D -m 755 ekos-@VERSION@-x86_64-unknown-linux-gnu/ekos ~/.ekos/bin/ekos
mkdir -p ~/.ekos/data
```

If `ekos --version` then fails with `version 'GLIBC_2.x' not found`, use the `musl` archive the
same way.

### Copying it to another machine

Copy only `ekos-@VERSION@-release.tar.gz` and repeat step 1 there. The binary does not depend on
the machine it was installed on.

## 2. First run

**On one repository, with the state kept inside it** (a `.ekos/` directory appears there):

```bash
cd /path/to/your/repo
ekos init --detect     # writes an ekos.toml that matches what is in this repository
ekos doctor
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
ekos coverage          # each kind of input produced objects
ekos query find "customer"
```

**As a project in the EKOS home, with the sources only read** (nothing is written into them):

```bash
ekos project add shop --source api=/repos/api --source handbook=/srv/handbook
ekos --project shop build
ekos --project shop recover && ekos --project shop resolve
ekos --project shop compile && ekos --project shop commit
ekos --project shop query find "customer"
```

The project's configuration and ledger are in `~/.ekos/data/default/shop/`. Edit its
`ekos.toml` to set a SQL dialect, exclusions or a model.

**Models.** The pipeline runs without any model. Passes that write descriptions or read
documents with a model are switched on in `ekos.toml` (`[llm]`). A hosted provider is paid per
call; a local Ollama server is not.

## 3. Connect an agent

```bash
ekos mcp serve --project shop                        # stdio, for Claude Code / Claude Desktop
ekos mcp serve --project shop --http 127.0.0.1:7331  # HTTP, for VS Code and other URL clients
```

For Claude Code, add it once:

```bash
claude mcp add ekos -- ekos mcp serve --project shop
```

Every tool except the two review tools only reads.

## 4. Upgrade and remove

- **Upgrade:** install a newer package the same way. The binary is replaced; projects in
  `~/.ekos/data` are kept and read by the new version. Take a copy first:
  `ekos home backup` writes the whole home to `~/.ekos/backups/`.
- **Remove:** `rm ~/.ekos/bin/ekos`. Projects stay in `~/.ekos`, and a repository used in place
  keeps its `.ekos/` directory; delete those yourself if you no longer want them.

## A team server

This package is the command-line tool. For a shared server with sign-in, tenants, roles and an
MCP endpoint for agents, run the container image instead; see `docs/deployment.md` in the
repository (<https://github.com/alexeyban/EKOS/blob/main/docs/deployment.md>).

## When something goes wrong

| Symptom | Cause and fix |
|---|---|
| `checksum mismatch` or `not listed in SHA256SUMS` | The archive is damaged or not the one this package lists. Copy the package again; do not install it |
| `version 'GLIBC_2.x' not found` | The system is older than glibc 2.34. The installer switches to the static build by itself; by hand, use the `musl` archive |
| `ekos: command not found` | `~/.ekos/bin` is not on `PATH`. Add the `export PATH=...` line above to `~/.bashrc` or `~/.profile` |
| `ekos coverage` reports a kind of input with no objects | Usually a wrong SQL dialect or an exclusion. `ekos init --detect --dry-run` shows what it would set |
