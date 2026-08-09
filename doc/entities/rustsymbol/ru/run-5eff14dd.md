# run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → ckm_rel_to_kir (`d4e53d27-cd91-5290-b888-f803751ddc3e`)
- → ckm_object_to_kir (`5d82f3c1-9c9a-501b-8527-e3d1a1305aa7`)
- → evidence_record_to_kir (`888fe357-b804-5d09-8f1e-26c4d5c7a9f3`)
- → open_ledger (`1bc6e585-c4e8-5dc5-a6c9-93aadb6ac859`)

### Contains

- ← ekos/crates/cli/src/commands/commit.rs (`f48ae11b-a9a7-54f0-8cc6-a192b1641436`)

## Diagram

```mermaid
graph TD
    n5eff14dd0262599c99883daeffd5ed67["run"]
    nf48ae11ba9a754f08cc6a192b1641436["ekos/crates/cli/src/commands/commit.rs"]
    nf48ae11ba9a754f08cc6a192b1641436 -->|Contains| n5eff14dd0262599c99883daeffd5ed67
    nd4e53d27cd915290b888f803751ddc3e["ckm_rel_to_kir"]
    n5eff14dd0262599c99883daeffd5ed67 -->|Calls| nd4e53d27cd915290b888f803751ddc3e
    n5d82f3c19c9a501b8527e3d1a1305aa7["ckm_object_to_kir"]
    n5eff14dd0262599c99883daeffd5ed67 -->|Calls| n5d82f3c19c9a501b8527e3d1a1305aa7
    n888fe357b8045d098f1e26c4d5c7a9f3["evidence_record_to_kir"]
    n5eff14dd0262599c99883daeffd5ed67 -->|Calls| n888fe357b8045d098f1e26c4d5c7a9f3
    n1bc6e585c4e85dc5a6c993aadb6ac859["open_ledger"]
    n5eff14dd0262599c99883daeffd5ed67 -->|Calls| n1bc6e585c4e85dc5a6c993aadb6ac859
```

## Evidence

_No evidence cited._
