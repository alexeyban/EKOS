# Lexer::tokenize (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Lexer::read_string (`8ce07f8f-7522-52d3-9c6e-69bb655cf743`)
- → Lexer::skip_whitespace (`3b1864e4-2e2f-5e9c-b6de-22f9d09c4d29`)
- → Lexer::read_ident (`923bdc64-064e-5247-a4bd-f16959f046a3`)
- → Lexer::read_number (`bdbed412-5bba-5cd2-a9f6-af805e8a049f`)
- → Lexer::match_symbol_op (`108c4fe6-d104-55b2-b854-d4e4125c8130`)
- ← ekl_parse (`590311ca-281e-528a-ae35-6f89aceae476`)

### Contains

- ← ekos/crates/ekl/src/parser.rs (`7eda2531-87c8-5048-92b2-c98483606431`)

## Diagram

```mermaid
graph TD
    n75766aca22b959f6aa0b7e6588ae0af9["Lexer::tokenize"]
    n7eda253187c8504892b2c98483606431["ekos/crates/ekl/src/parser.rs"]
    n7eda253187c8504892b2c98483606431 -->|Contains| n75766aca22b959f6aa0b7e6588ae0af9
    n8ce07f8f752252d39c6e69bb655cf743["Lexer::read_string"]
    n75766aca22b959f6aa0b7e6588ae0af9 -->|Calls| n8ce07f8f752252d39c6e69bb655cf743
    n3b1864e42e2f5e9cb6de22f9d09c4d29["Lexer::skip_whitespace"]
    n75766aca22b959f6aa0b7e6588ae0af9 -->|Calls| n3b1864e42e2f5e9cb6de22f9d09c4d29
    n923bdc64064e5247a4bdf16959f046a3["Lexer::read_ident"]
    n75766aca22b959f6aa0b7e6588ae0af9 -->|Calls| n923bdc64064e5247a4bdf16959f046a3
    nbdbed4125bba5cd2a9f6af805e8a049f["Lexer::read_number"]
    n75766aca22b959f6aa0b7e6588ae0af9 -->|Calls| nbdbed4125bba5cd2a9f6af805e8a049f
    n108c4fe6d10455b2b854d4e4125c8130["Lexer::match_symbol_op"]
    n75766aca22b959f6aa0b7e6588ae0af9 -->|Calls| n108c4fe6d10455b2b854d4e4125c8130
    n590311ca281e528aae356f89aceae476["ekl_parse"]
    n590311ca281e528aae356f89aceae476 -->|Calls| n75766aca22b959f6aa0b7e6588ae0af9
```

## Evidence

_No evidence cited._
