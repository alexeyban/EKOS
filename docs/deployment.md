# Deploying EKOS for a team

One server, one container, one volume. This guide takes you from nothing to a console that
several people and agents use, with separate tenants, roles, and backups.

It describes what exists today. What is not built is listed at the end; read that before
planning around it.

## What you are deploying

| Piece | What it is |
|---|---|
| The image | The `ekos` binary, the console API and the console UI, on port 8000 |
| The EKOS home | One volume, `/var/lib/ekos`: every project's config and compiled data, the console's database and run logs. **This is all the state there is** |
| Your sources | Repositories and document folders on the host, mounted read-only at `/sources`. EKOS never writes to them |
| An identity provider | Any OIDC provider (Keycloak, Entra ID, Okta, Google). EKOS stores no passwords |
| A reverse proxy | For TLS. The container speaks plain HTTP on loopback |

Levels of organisation: **tenant → project → source → object**. A tenant is a team or a
customer. A project is one ledger. A source is one repository or folder of a project.

## 1. First start

```bash
git clone https://github.com/alexeyban/EKOS && cd EKOS
cp deploy/.env.example deploy/.env
```

In `deploy/.env` set at least:

```bash
EKOS_SOURCES=/srv/repos                 # the host directory holding your sources
EKOS_CONSOLE_SESSION_SECRET=...         # openssl rand -hex 32

EKOS_CONSOLE_OIDC_ISSUER=https://id.example.com/realms/main
EKOS_CONSOLE_OIDC_CLIENT_ID=ekos
EKOS_CONSOLE_OIDC_CLIENT_SECRET=...
EKOS_CONSOLE_OIDC_REDIRECT_URI=https://ekos.example.com/api/auth/callback

EKOS_CONSOLE_OWNERS=you@example.com     # platform owners, comma-separated
EKOS_CONSOLE_PRODUCTION=true
```

```bash
docker compose -f deploy/docker-compose.yml up -d --build
```

Tagged releases publish `ghcr.io/alexeyban/ekos:<version>`; until the next tag, `--build` builds
the image from your checkout. The build compiles the binary and takes several minutes.

### A server without internet access

Build the image on a machine that has access, save it, and load it on the server:

```bash
# on the connected machine, in the checkout
docker compose -f deploy/docker-compose.yml build          # tags ghcr.io/alexeyban/ekos:latest
docker save ghcr.io/alexeyban/ekos:latest | gzip > ekos-image.tar.gz
tar czf ekos-deploy.tar.gz deploy/docker-compose.yml deploy/.env.example docs/deployment.md

# on the server
docker load < ekos-image.tar.gz
tar xzf ekos-deploy.tar.gz && cp deploy/.env.example deploy/.env    # then edit it as above
docker compose -f deploy/docker-compose.yml up -d                   # no --build
```

Compose uses the loaded image because one with that name exists locally; it pulls only when
none does. The identity provider must still be reachable from the server.

### A server without Docker

The full package (RFC 0185) runs the same console from its own bundled Python. On the server,
as the user that will own the home:

```bash
cat > answers.toml <<'EOF'
[console]
mode = "team"
host = "127.0.0.1"
port = 8000
oidc_issuer = "https://id.example.com/realms/main"
oidc_client_id = "ekos"
oidc_redirect_uri = "https://ekos.example.com/api/auth/callback"
owners = "you@example.com"
production = true
[start]
at_login = true      # a systemd user unit; `loginctl enable-linger <user>` keeps it running
open = false
EOF
bin/ekos setup --answers answers.toml
```

The OIDC client secret goes in the keychain where the server has one (`oidc_client_secret` in
the answers), otherwise in the user unit's environment (`EKOS_CONSOLE_OIDC_CLIENT_SECRET`).
Without a session secret the console keeps a generated one in `control/session.key`, readable by
its owner only, so sessions survive a restart. Settings changed later on the console's Settings page
apply after **Restart now**, which works here because `ekos console run` starts it again.

To remove a server installed this way, run `bin/ekos uninstall` on it (`--purge --yes` also
deletes the home). The console's **Settings → Uninstall EKOS** page is offered in local mode
only: it opens a page on the console's own machine, which a team's owner reaches from elsewhere.

People who want the `ekos` command on their own machine (for a project of their own, or for
`ekos mcp serve` beside an editor) do not need the image: `scripts/package-release.sh` builds an
offline install package for Linux x86_64 with the installer and an `INSTALL.md`; see the
README's [Installation](../README.md#offline-from-an-install-package) section.

`EKOS_CONSOLE_PRODUCTION=true` makes the console refuse to start without OIDC, an owner and a
session secret, turns the two development tokens off, and gives nobody access except through a
granted role. Without it the console is a development setup: static tokens work, and every
signed-in user can read the `default` tenant.

Put a reverse proxy with TLS in front of `127.0.0.1:8000`. Do not change the port binding to
`0.0.0.0`.

The container must be able to reach the identity provider at the issuer URL, and that URL must
be the same one browsers use. If sign-in answers "the identity provider could not be reached",
check that from inside the container. With an `https://` redirect URI the session cookie is
marked `Secure`.

**To try it without an identity provider**, leave the OIDC and production lines out and set
`EKOS_CONSOLE_CONSOLE_TOKEN` and `EKOS_CONSOLE_CONSOLE_WRITE_TOKEN` instead. The write token acts
as a platform owner. Do not expose such a console.

### Changing settings later

A platform owner can change the console's settings on its **Settings** page: sign-in, owners,
MCP limits and the rest. They are saved in `/var/lib/ekos/control/console.toml`, in the volume.
A variable set in `deploy/.env` wins over the page and is shown there read-only, so the
deployment file stays the authority for what it sets. Settings apply when the console starts
again; the page's **Restart now** works because the Compose restart policy starts it again.

Secrets (the OIDC client secret, the session secret) cannot be typed on that page in a
container: they go to an OS keychain, and a container has none. Keep them in `deploy/.env`.

## 2. Tenants

Sign in as an owner and open **Admin**.

- **Create a tenant** for each team or customer. Its id is lowercase letters, digits, `-`, `_`.
- **Set its source roots**: the directories under `/sources` its projects may read, one per
  line. A tenant with none can create nothing. Only an owner sets them, so a tenant's admin
  cannot point a project at another team's repository.

The tenant `default` exists from the start. In production mode it follows the same rules as any
other.

## 3. People and roles

Grant a role to an e-mail address in **Admin → Roles**. The person gets it at their first
sign-in with that address.

| Role | May |
|---|---|
| `viewer` | read everything in a project, including runs and logs |
| `reviewer` | also decide business semantics and conflicts |
| `operator` | also run the pipeline, manage schedules, edit the project config |
| `admin` | everything in the tenant: projects, members, groups, tokens, audit log |

- A role is granted on the whole tenant or on one project. `admin` is tenant-wide.
- `reviewer` and `operator` are independent. Give both to someone who does both.
- A **group** holds a role for its members. Give it a claim value and everyone whose sign-in
  carries that value in the provider's group claim (`EKOS_CONSOLE_OIDC_ROLE_CLAIM`, default
  `groups`) is a member without being listed.
  Many providers (Dex, Keycloak, Okta) send that claim only when asked: set
  `EKOS_CONSOLE_OIDC_SCOPES=openid email profile groups`. Without it such a group stays empty
  and nothing says why.
- A project or tenant you have no role in looks exactly like one that does not exist.

Removing a role takes effect on that person's next request.

## 4. Projects

A tenant admin adds a project in **Add a workspace → New project from a source directory**,
with a path such as `/sources/payments-api`. Then an operator presses **Init & compile** in the project's
bar (RFC 0189: build, recover, resolve, compile, commit and a coverage report as one run; identity
conflicts are reported in the log and left for review), or runs single stages from the **Run** tab. Each stage of a chained run may take up to six
hours (`run_stage_timeout_seconds` on the Settings page); a single command keeps a 30-minute
limit.

A project of several repositories and folders is created from the command line:

```bash
docker compose -f deploy/docker-compose.yml exec ekos \
  ekos --tenant acme project add shop \
    --source api=/sources/payments-api --source web=/sources/storefront \
    --source handbook=/sources/handbook
```

It appears in the console after a restart. Naming the sources is what lets you limit access by
source later, and a project compiled without names cannot gain them.

## 5. Limits inside a project

A viewer can be limited to part of one project. In **Admin → Roles**, grant `viewer` on one
workspace and choose:

- **only…** a source, or a path pattern in it (`src/**`);
- **all except…** a source or a path pattern (`secrets/**`).

Know what this does before relying on it:

- A limited person gets the graph, search, object pages and impact analysis, filtered. The
  dashboard, run history and logs, the configuration, semantics and conflicts are closed to
  them, because those show more than the ledger.
- Anything the compiler could not tie to a source is hidden from them: commit authors, crates,
  rollups, business-semantics items, conflicts.
- Operators and admins cannot be limited. Running the pipeline shows every source.
- A rule hides objects. It does not rewrite a visible document that mentions a hidden one.

## 6. Agents

Create an API token in **Admin → API tokens**. It is shown once. A token is `viewer` or
`operator`, on the tenant or one project, and can carry the same limits as a person. It can
never review or administer.

An MCP client connects to the project through the console:

```json
{"url": "https://ekos.example.com/mcp/acme/shop",
 "headers": {"Authorization": "Bearer ekos_…"}}
```

The agent sees the tools its role allows, limited to what its rules let it see. Every call is
in the tenant's audit log: who, which tool, allowed or denied. Arguments are not recorded.

A project's MCP server starts on first use and stops after 15 idle minutes; at most 8 run at
once (`EKOS_CONSOLE_MCP_IDLE_SECONDS`, `EKOS_CONSOLE_MCP_MAX_SERVERS`). Each holds its project's
index in memory, so size the cap to the machine.

### Extension pages

A console started by an extension's binary (`ekos-lakehouse console start`, or `EKOS_BIN`
pointing at it) shows that extension's pages as extra workspace tabs (RFC 0198). A page is data
the binary declares: panels from its read commands, buttons that run its catalogue commands with
their usual class, role and confirmation. A restricted reader does not see these tabs. A job a
button starts has its tenant's environment, like every other job: a secret it needs (a source
database's password) is granted to the tenant below, or kept in an owner-only file the project
names — EKOS Lakehouse's installer does the latter
([its docs](https://github.com/alexeyban/ekos-lakehouse/blob/main/docs/console.md)).

## 7. Model credentials

Some pipeline passes call a language model. A project's `ekos.toml` names the environment
variable holding the key; it never holds the key.

1. Put the keys in `deploy/.env`, one variable per tenant that pays for its own:

   ```bash
   ACME_ANTHROPIC_KEY=sk-ant-...
   GLOBEX_OPENAI_KEY=sk-...
   ```

2. In **Admin → Tenants**, under *Model credentials*, list the variable names for each tenant
   and tick **Allow metered model calls**.

A tenant's pipeline and MCP server receive its own named variables and no other tenant's, and
none of the console's settings. With the box unticked they receive none, so a hosted provider
fails for want of a key. A local model server still works: point `ekos.toml` at
`http://host.docker.internal:11434`.

Calls to a hosted model are billed by that provider. EKOS does not track or cap spending.

Outside production mode the `default` tenant keeps the console's whole environment (minus the
console's own settings and other tenants' named variables), as before this existed.

### The chat model

The console's Chat tab (RFC 0187) uses one model, set by an owner in **Settings → Chat**:
`chat_provider` (`ollama`, `openai`, `anthropic`), `chat_model`, `chat_base_url` and
`chat_api_key_env`, the name of the variable holding the key. In a container put the key itself
in `deploy/.env` under that name. A hosted model is paid per call: a tenant's people can chat
with it only while that tenant's metered calls are on (**Admin → Tenants**), the same switch as
for its projects' model calls. A model on the server itself (Ollama, a local OpenAI-compatible
server) always may.

## 8. Backup and restore

Everything is in the volume. A backup is one file.

```bash
# every tenant, and the console's database
docker compose -f deploy/docker-compose.yml exec ekos ekos home backup
# → /var/lib/ekos/backups/ekos-backup-<time>.tar.zst

# one tenant's projects only
docker compose -f deploy/docker-compose.yml exec ekos ekos --tenant acme home backup
```

Copy the file off the machine: it is written inside the volume it backs up.

```bash
docker compose -f deploy/docker-compose.yml cp \
  ekos:/var/lib/ekos/backups/ekos-backup-20261013T101500Z.tar.zst /srv/backups/
```

- A project is copied under its ledger's write lock. If a pipeline stage is writing, the backup
  stops with that project's name and writes nothing; run it again afterwards. Reads, including
  agents' queries, are not interrupted.
- The console database is copied as a consistent snapshot while the console runs.
- Not in a backup: your sources (they are not in the volume), the binary, the trash.
- The archive is not encrypted. It holds the ledgers and the console database (token hashes,
  not tokens). Protect it as you would the volume.
- There is no schedule. Run it from the host's cron.

To restore onto a new machine or a new volume, start the deployment first, then:

```bash
docker compose -f deploy/docker-compose.yml cp /srv/backups/ekos-backup-….tar.zst ekos:/tmp/b.tar.zst
docker compose -f deploy/docker-compose.yml exec ekos ekos home restore /tmp/b.tar.zst --force
docker compose -f deploy/docker-compose.yml restart
```

`--force` is needed even on a new volume: the console created its own empty database when it
started, and a full backup holds a console database. The empty one is moved to `trash/`.
Restart afterwards; until then the console is still using the database it started with.

- Without `--force` nothing that exists is replaced. If any project in the archive, or the
  console database, is already there, the restore stops and changes nothing.
- `--force` replaces: each existing project, and the console database, is first moved to
  `trash/` in the volume. Nothing is deleted.
- `--tenant acme` restores that tenant's projects only. Role bindings are in the console
  database, which a per-tenant archive does not include.
- A project's config holds the path of its sources. Restore onto a machine where the sources
  are mounted at the same path, or edit the project's `ekos.toml`.

## 9. Upgrading

```bash
docker compose -f deploy/docker-compose.yml exec ekos ekos home backup
git pull                                   # or change the image tag
docker compose -f deploy/docker-compose.yml up -d --build
```

- The console upgrades its own database at start. Take the backup first: there is no downgrade.
- A ledger is append-only and is read by newer versions. Some upgrades change what is indexed
  and rebuild the search index at the first write; the release notes say when.
- Re-running the pipeline is never needed for an upgrade. It is how new compiler features reach
  an existing project, and for a project with model-backed passes it costs money.

## What is not built

- **One server only.** No Kubernetes manifests, no PostgreSQL, no object storage for the home.
- **Isolation between tenants is by directory and permission check**, inside one process and
  one OS user. It is not a sandbox.
- **No TLS in the container**, no OAuth for MCP clients, no rate limits, no spending limits.
- **No scheduled or encrypted backups**, and no deletion of a tenant or a user.
- **Limits inside a project** leave a limited reader without a dashboard, semantics or
  conflicts, and cannot apply to operators.
- **Sources are local directories.** EKOS does not clone from a URL; mount a checkout.
- **Evaluation runs** (`ekos eval run`) write beside the sources and do not work for a project
  kept in the home.
- The image is `linux/amd64`.

## Where things are

| | |
|---|---|
| A project | `/var/lib/ekos/data/<tenant>/<project>/` (`ekos.toml`, `state/`) |
| Console database, run logs | `/var/lib/ekos/control/` |
| Removed or replaced projects | `/var/lib/ekos/trash/` |
| Backups | `/var/lib/ekos/backups/` |
| Settings | `deploy/.env`; every one is described in `deploy/.env.example` |
| Design | RFCs 0178–0183 in `ekos/docs/rfcs/` |
| An offline install package of the CLI | `scripts/package-release.sh` → `dist/ekos-<version>-release.tar.gz` |
| A check of a whole deployment | `deploy/e2e/run.sh`: the image, a test identity provider, two tenants, a group from a claim, an MCP client, the pages in a browser, backup and restore |
