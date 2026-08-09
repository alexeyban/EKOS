# generate (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → generate_curated (`5a70d7a9-bb4c-59dc-a7cd-00be6ab7f553`)
- → confirm_prose_spend (`a41df42a-3316-58c5-82d0-b80aedac6aad`)
- → select_llm_provider_for_prose (`e9f6c656-18e6-51d4-a612-415a39136d77`)
- → enrich_with_prose (`3cdfa189-a68e-5bf7-a3c8-0f7579e2fc62`)
- → write_page (`6672efa9-4ce8-5c9c-a096-3c74f2963490`)
- → render_er_diagram_page (`76d267df-6bbf-58bb-8682-5bd99c95a7c2`)

### Contains

- ← ekos/crates/cli/src/commands/docs.rs (`5503d15b-112f-541f-8189-8be05a060beb`)

## Diagram

```mermaid
graph TD
    n9628a7cf316d540082616b2216ee01f1["generate"]
    n5503d15b112f541f81898be05a060beb["ekos/crates/cli/src/commands/docs.rs"]
    n5503d15b112f541f81898be05a060beb -->|Contains| n9628a7cf316d540082616b2216ee01f1
    n5a70d7a9bb4c59dca7cd00be6ab7f553["generate_curated"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n5a70d7a9bb4c59dca7cd00be6ab7f553
    na41df42a331658c582d0b80aedac6aad["confirm_prose_spend"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| na41df42a331658c582d0b80aedac6aad
    ne9f6c65618e651d4a612415a39136d77["select_llm_provider_for_prose"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| ne9f6c65618e651d4a612415a39136d77
    n3cdfa189a68e5bf7a3c80f7579e2fc62["enrich_with_prose"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n3cdfa189a68e5bf7a3c80f7579e2fc62
    n6672efa94ce85c9ca0963c74f2963490["write_page"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n6672efa94ce85c9ca0963c74f2963490
    n76d267df6bbf58bb86825bd99c95a7c2["render_er_diagram_page"]
    n9628a7cf316d540082616b2216ee01f1 -->|Calls| n76d267df6bbf58bb86825bd99c95a7c2
```

## Evidence

_No evidence cited._
