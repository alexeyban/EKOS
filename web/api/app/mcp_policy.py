"""Which MCP tools a caller may reach through the gateway (RFC 0182).

`ekos mcp serve` knows nothing of roles. The gateway decides from the tool's name what right a
call needs, and for a caller who sees part of a project which tools exist at all.
"""

from __future__ import annotations

# The tools that read knowledge only through the ledger, which a read scope filters (RFC 0181).
# The same list as `SCOPED_TOOLS` in `ekos/crates/cli/src/commands/mcp.rs`; a test reads that
# file and fails when the two differ. The server enforces its own copy: this one keeps a
# restricted caller's tool list true and refuses early.
SCOPED_TOOLS = frozenset(
    {
        "ekos_search",
        "ekos_query",
        "ekos_retrieve",
        "ekos_ekl",
        "ekos_neighborhood",
        "ekos_state",
        "ekos_dependents",
        "ekos_impact",
        "ekos_diff",
        "ekos_graph_export",
    }
)

# Review decisions: a signed-in person with the `reviewer` role. No API token can hold it
# (RFC 0180), so no agent reaches these through the gateway.
REVIEW_TOOLS = frozenset({"ekos_identity_review", "ekos_architecture_review"})

# Writes that are not review decisions. `ekos_session_note` appends to the session inbox.
OPERATE_TOOLS = frozenset({"ekos_session_note"})


def action_for(tool: str) -> str:
    """The right (RFC 0180) a call to `tool` needs. A tool this module has never heard of is
    treated as a read; a restricted caller cannot reach it anyway, since it is not scoped."""
    if tool in REVIEW_TOOLS:
        return "review"
    if tool in OPERATE_TOOLS:
        return "operate"
    return "read"
