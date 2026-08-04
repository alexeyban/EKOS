# Marketing Agent v1

RFC 0030 (`ekos/docs/rfcs/0030-marketing-agent.md`). Turns a new `devlog_N.md` into a
human-approved X (Twitter) release announcement, posted to the project's official account,
[@ekosproject](https://x.com/ekosproject).

```bash
cargo run -p ekos -- marketing publish            # latest devlog, interactive approval
cargo run -p ekos -- marketing publish 28          # a specific devlog number
cargo run -p ekos -- marketing publish --dry-run   # preview only, never posts, never records
cargo run -p ekos -- marketing publish --yes       # skip the approval prompt (still gated by
                                                    # [marketing.twitter] enabled in ekos.toml)
```

## Configuration

`[marketing]` in `ekos.toml` at the repo root (not a separate file — see RFC 0030's Motivation
for why this deviates from the source design doc's `marketing/config.yaml`):

```toml
[marketing]
github = "https://github.com/alexeyban/EKOS"
hashtags = ["Rust", "AI", "MCP"]

[marketing.twitter]
enabled = false   # must be explicitly turned on to actually post
dry-run = false
```

Publishing to X requires `TWITTER_API_KEY`, `TWITTER_API_SECRET`, `TWITTER_ACCESS_TOKEN`,
`TWITTER_ACCESS_SECRET` in the environment (OAuth 1.0a user-context tokens from the X Developer
Portal, "Read and write" permissions). Never commit these. There is no separate "target account"
setting — OAuth 1.0a user-context tokens are already tied to one account, so whichever
credentials are configured determine where a post lands. To publish as
[@ekosproject](https://x.com/ekosproject), the tokens must come from that account's Developer
Portal app.

## What lives in this directory

Everything under `marketing/` other than this README and `templates/` is runtime state, created
on first use:

| Path | Contents |
|---|---|
| `posted/tweets.json` | Duplicate-detection ledger — one entry per published devlog |
| `logs/marketing.log` | Plain-text run log (one line per step: read, importance, generated, approved, published) |

The actual logic (parsing, importance classification, prompt construction, validation,
publishing) lives in `ekos/crates/marketing/` — this directory is state and docs only.
