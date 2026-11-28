"""Build a prompt context for a language model from compiled, cited knowledge.

    python grounded_context.py <workspace> "<question>"

`kb.query` answers from the ledger alone: no model is called, and each claim names the evidence
it rests on. This script turns the claims into a numbered context block you can put in front of
any model, so its answer can cite [1], [2], ... and every citation resolves to a file.

Claims that sources disagree about are marked, so the model is not handed one side as fact.
"""

from __future__ import annotations

import sys

import ekos


def context_for(kb: ekos.Knowledge, question: str, limit: int = 12) -> str:
    answer = kb.query(question)
    # A weak claim is a partial match: a candidate, not support for the question's premise.
    claims = [claim for claim in answer.claims if not claim.weak][:limit]
    if not claims:
        return "No compiled knowledge answers this question."

    lines = ["Facts compiled from the sources (cite them by number):"]
    for number, claim in enumerate(claims, start=1):
        source = claim.evidence[0].location if claim.evidence else "no evidence recorded"
        flag = "  [DISPUTED: sources disagree, nobody has decided]" if claim.disputed else ""
        lines.append(f"[{number}] {claim.text}  (source: {source}){flag}")
    return "\n".join(lines)


def main(workspace: str, question: str) -> None:
    kb = ekos.open(workspace)
    context = context_for(kb, question)
    prompt = (
        f"{context}\n\n"
        "Answer the question using only the facts above, citing them as [n]. "
        "If they do not answer it, say so.\n\n"
        f"Question: {question}"
    )
    print(prompt)  # hand this to the model of your choice


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
