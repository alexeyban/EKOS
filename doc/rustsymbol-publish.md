# publish (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → log_line (`9b3a0105-69f6-541d-ab3c-6c9ae99908ff`)
- → approve (`76efc638-276d-5610-966b-18f8fc582a09`)
- → select_llm_provider (`824776cf-1030-5c02-acf6-961e14b329e1`)
- → resolve_devlog_path (`8cf1b866-178a-5ca7-b5dd-40b1aa8e7de7`)

### Contains

- ← ekos/crates/cli/src/commands/marketing.rs (`e4550c2d-5dcf-5779-b25d-ac86e4019342`)

## Diagram

```mermaid
graph TD
    ne7c2f511275951deb835a5aa57064c4f["publish"]
    ne4550c2d5dcf5779b25dac86e4019342["ekos/crates/cli/src/commands/marketing.rs"]
    ne4550c2d5dcf5779b25dac86e4019342 -->|Contains| ne7c2f511275951deb835a5aa57064c4f
    n9b3a010569f6541dab3c6c9ae99908ff["log_line"]
    ne7c2f511275951deb835a5aa57064c4f -->|Calls| n9b3a010569f6541dab3c6c9ae99908ff
    n76efc638276d5610966b18f8fc582a09["approve"]
    ne7c2f511275951deb835a5aa57064c4f -->|Calls| n76efc638276d5610966b18f8fc582a09
    n824776cf10305c02acf6961e14b329e1["select_llm_provider"]
    ne7c2f511275951deb835a5aa57064c4f -->|Calls| n824776cf10305c02acf6961e14b329e1
    n8cf1b866178a5ca7b5dd40b1aa8e7de7["resolve_devlog_path"]
    ne7c2f511275951deb835a5aa57064c4f -->|Calls| n8cf1b866178a5ca7b5dd40b1aa8e7de7
```

## Evidence

_No evidence cited._
