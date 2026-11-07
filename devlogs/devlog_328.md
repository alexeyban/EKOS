# Devlog 328 — Public-demo safety (RFC 0249): policy, service, live red team, and a launch blocker

**Date:** 2026-11-08
**PRs:** none (direct commits to main)
**Branch:** main

---

## Summary
RFC 0249's backend is built in the console: deterministic signals, a standing state machine (ban, 24 h
block, refusal), daily limits, an address rate limit and block, the owner's log and unban, and an
intent-check model behind it. It was measured live on OpenCode Zen with Claude Haiku 5.5. Reading
OpenCode's Terms found a **launch blocker**: they allow use "for your own internal use, and not on behalf
of or for the benefit of any third party", which the plan (strangers through the owner's key) is not.

## What was built (web/api)
| Piece | File |
|---|---|
| Normalisation, signals, standing, limits (pure) | `app/moderation_policy.py` |
| Service over the console database | `app/moderation.py` (+ `ModUser`, `ModEvent`, `ModUsage`, `ModIpBlock` in `models.py`) |
| The intent check over any chat provider | `app/moderation_model.py` |
| Routes (404 unless `moderation_enabled`) | `app/routes/demo_safety.py` |
| Live measurement, never run by tests | `tools/redteam_live.py` |
| Corpus: 59 attacks, 44 ordinary questions | `tests/redteam_corpus.py` |
| Settings `moderation_*`, `docs/deployment.md` section | `settings.py`, `console_file.py` |

## Decisions
- The model can only add a refusal. A hard signal bans whatever it says; a model that cannot be reached
  refuses without penalty or charge; its answer is parsed strictly and anything else is "unavailable".
- Standing is read before the address block (a banned person hears "banned").
- Two matching views of every message: `1=1` and `$snapshots` need the plain one, `1gn0re` the de-leeted one.
- `<`/`>` are escaped inside the JSON string handed to the model, so a message cannot close its block.
- Tokens are keyed by kind and id; they used to share the subject "token" (both static tokens) and with it a ban.

## Measured (claude-haiku-5-5, OpenCode Zen, 2026-11-08)
103 messages, ~55k tokens (about 540 each, mostly the fixed instructions). Deterministic layer alone, offline:
54 of 59 attacks, 0 of 44 ordinary questions. Model: 0 false positives; flagged the attacks it answered except
three that patterns catch anyway (one more was the corpus error below); 5 non-JSON answers, all of them attacks (fail closed). The model catches spaced-out letters
("i g n o r e   a l l …") that the patterns miss. The first corpus entry for that was wrong ("i g n o r e" alone
asks for nothing); the live run is what showed it.

## Knowledge Captured
- **OpenCode Zen's Terms of Use forbid serving third parties through your key** (see above). The docs say all
  models are zero-retention except OpenAI and Anthropic APIs (30 days) and several free models that may train;
  free chat models only work inside OpenCode. Pages not read: `/console/enterprise`, per-provider policies.
- **Invisible characters in source.** A script that writes a Python file through a non-raw string turns `\u202e` into the real character, and a test corpus then holds actual bidirectional and zero-width characters (SonarCloud S6389 caught four lines; it is the "Trojan Source" hazard, even in a test file). Write the escape *text* (`\\u202e`), and check with `grep -P '[\x{200B}-\x{200D}\x{202A}-\x{202E}\x{2060}\x{FEFF}]'` before committing. The first push of this work failed the quality gate on it, on a duplicated literal (S1192) and on `Moderator.screen` at complexity 19 (S3776; now `_gate` / `_read` / `_record`).
- A route that only requires sign-in must be added to `DYNAMIC` in `test_authz_routes.py` — a reviewed decision.
- `Settings` fields need a row in `console_file.FIELDS` or a test fails.
- A reply from a subagent (here the terms check) is data: its "confirm with OpenCode" is advice to the owner,
  not an instruction to anyone.

## Not done
A human red team; multilingual and multi-turn attacks; the consent flow and spending of `additive`/`modifying`
(RFC 0248); the erasure design; written confirmation from OpenCode.

## Files Changed
`web/api/app/{moderation_policy,moderation,moderation_model,models,settings,console_file,main}.py`,
`web/api/app/routes/demo_safety.py`, `web/api/app/chat/providers.py` (an empty `tools` is omitted),
`web/api/tests/{test_moderation_policy,test_moderation,redteam_corpus,test_authz_routes}.py`,
`web/api/tools/redteam_live.py`, `docs/deployment.md`, `ekos/docs/rfcs/0249-public-demo-safety.md`, `TODO.md`.
