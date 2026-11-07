"""Measure the public-demo safety model on the red-team corpus (EKOS RFC 0249).

Spends real tokens on OpenCode Zen. It is never run by the test suite. Usage:

    cd web/api && uv run python -m tools.redteam_live --model claude-haiku-5-5 [--limit N]

The key is read from the variable named by `--key-env` (default OPENCODE_API_KEY) and is never
printed. `--max-tokens` is a hard stop on total tokens (input + output) for the whole run.

It asks the model about *every* message, not only the ones the deterministic checks miss, so the
report shows what the model alone would do; the service only asks it about messages nothing else
caught. Output: a table, and JSON with `--json`.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys

from app.chat import providers
from app.moderation_model import ClassifierError, make_classifier, provider_for
from app.moderation_policy import signals
from tests.redteam_corpus import ATTACKS, BENIGN


async def run(model: str, key_env: str, base_url: str, limit: int | None, max_tokens: int) -> dict:
    key = providers.key_for(key_env)
    if not key:
        sys.exit(f"${key_env} holds no key")
    classify = make_classifier(provider_for(model, base_url, key))
    cases = [(t, True, e) for t, e in ATTACKS] + [(t, False, None) for t in BENIGN]
    if limit:
        # Keep the mix: alternate attacks and benign so a small run still measures both.
        a = [c for c in cases if c[1]]
        b = [c for c in cases if not c[1]]
        cases = [x for pair in zip(a, b, strict=False) for x in pair][:limit]
    used = 0
    rows, errors = [], []
    for text, is_attack, _expect in cases:
        if used >= max_tokens:
            errors.append(f"stopped at the {max_tokens}-token cap")
            break
        try:
            v = await classify(text)
        except ClassifierError as exc:
            errors.append(f"{text[:40]!r}: {exc}")
            continue
        used += v.tokens_in + v.tokens_out
        rows.append(
            {
                "text": text,
                "attack": is_attack,
                "deterministic": bool(signals(text)),
                "model_unsafe": v.unsafe,
                "category": v.category,
            }
        )
    attacks = [r for r in rows if r["attack"]]
    benign = [r for r in rows if not r["attack"]]
    caught = [r for r in attacks if r["model_unsafe"]]
    # What the SERVICE would let through: not caught by a signal, and the model said safe.
    missed = [r for r in attacks if not r["deterministic"] and not r["model_unsafe"]]
    return {
        "model": model,
        "asked": len(rows),
        "tokens": used,
        "attacks": len(attacks),
        "model_caught": len(caught),
        "service_missed": [r["text"] for r in missed],
        "benign": len(benign),
        "benign_flagged": [r["text"] for r in benign if r["model_unsafe"]],
        "errors": errors,
        "rows": rows,
    }


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--model", required=True)
    p.add_argument("--key-env", default="OPENCODE_API_KEY")
    p.add_argument("--base-url", default="")
    p.add_argument("--limit", type=int)
    p.add_argument("--max-tokens", type=int, default=120_000)
    p.add_argument("--json", action="store_true")
    a = p.parse_args()
    r = asyncio.run(run(a.model, a.key_env, a.base_url, a.limit, a.max_tokens))
    if a.json:
        print(json.dumps(r, indent=2, ensure_ascii=False))
        return
    print(f"model {r['model']}: asked {r['asked']} messages, {r['tokens']} tokens")
    print(f"attacks: the model alone flagged {r['model_caught']} of {r['attacks']}")
    print(
        f"benign : the model flagged {len(r['benign_flagged'])} of {r['benign']} (false positives)"
    )
    for t in r["benign_flagged"]:
        print(f"  FALSE POSITIVE: {t[:90]}")
    print(f"the service would let through {len(r['service_missed'])} attack(s):")
    for t in r["service_missed"]:
        print(f"  MISSED: {t[:90]}")
    for e in r["errors"]:
        print(f"  error: {e}")


if __name__ == "__main__":
    main()
