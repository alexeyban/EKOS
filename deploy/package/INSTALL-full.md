# Installing EKOS @VERSION@ (@TARGET@)

This package holds everything EKOS needs on this machine: the `ekos` command, the web console
and the Python that runs it. Nothing else has to be installed. It was built from commit
`@COMMIT@`.

## 1. Start the setup

Unpack the archive anywhere (your Downloads folder is fine), then:

| System | Do this |
|---|---|
| **Windows 11** | Double-click `setup.cmd`. If Windows says "Windows protected your PC", choose **More info → Run anyway**: the program is not code-signed. |
| **macOS** | Right-click `setup.command` → **Open**, then **Open** again. macOS asks the first time because the program is not notarised. |
| **Linux** | Run `./setup.sh` in a terminal. |

Setup opens a page in your browser. Nothing changes until you confirm on its last page.

## 2. What setup asks

1. **Location** — the EKOS home, where projects and their compiled knowledge live (default
   `~/.ekos`, on Windows `C:\Users\<you>\.ekos`), and where the program goes (default: inside
   the home). It can put `ekos` on your `PATH`.
2. **Web console** — *Only me* (local mode: this machine only, no sign-in provider) or *A team*
   (sign-in through your organisation's OIDC provider; a client secret you type goes to the
   system keychain, never to a file). The port, and limits for the per-project MCP servers.
3. **First project** — a name and the directories it reads (`name=directory`, one per line).
   EKOS never writes into them.
4. **Agents** — register the project's MCP server with Claude Code, Claude Desktop or VS Code.
   Each config file is backed up before it is changed, and other entries are kept.
5. **Chat** (optional) — a model that answers questions about a project with its EKOS tools: a
   local Ollama model (not paid per call; the installed ones are listed), Anthropic, or any
   OpenAI-compatible server. A key you type goes to the system keychain. "Check the model" reads
   the provider's model list and calls no model.
6. **Start** — start the console when you log in, start it now, open it.

The page then lists every step with its result.

## 3. Afterwards

```
ekos console open      # open the console (in local mode: signed in with a one-time code)
ekos console status    # is it running, where
ekos console stop | start | restart | logs
ekos setup             # change any of the above later
```

Compile a project from the console — the **Init & compile** button in the project's bar runs
every step — or from a terminal:

```
ekos --project shop build && ekos --project shop recover && ekos --project shop resolve
ekos --project shop compile && ekos --project shop commit
```

Every setting of the console is on its **Settings** page (signed in as an owner); a project's
`ekos.toml` is on the project's **Config** page.

## 4. Removing EKOS

```
ekos uninstall               # the program, the login entry, the PATH line, agent entries
ekos uninstall --purge --yes # also every project and the console's database
```

Or in the browser: the console's **Settings → Uninstall EKOS**, or `ekos uninstall --ui`. The
page lists what will be removed and your projects with their size; you choose whether to keep
the projects and type a word to confirm. Nothing is removed before that.

## Without the setup page

Write the answers to a file and apply them, for a server or a script:

```toml
# answers.toml
add_to_path = true
[console]
mode = "local"
port = 8000
[project]
name = "shop"
sources = ["/home/me/repos/shop"]
[start]
at_login = true
```

```
bin/ekos setup --answers answers.toml --dry-run   # what it would do
bin/ekos setup --answers answers.toml
```
