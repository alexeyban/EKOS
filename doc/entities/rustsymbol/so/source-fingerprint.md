# source_fingerprint (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → ScanContext::is_ignored (`8b666d11-3ab8-52b1-8b71-7cb270682a0b`)
- → ObservationPackage::push (`dd106f4a-4759-564d-835a-4d25afcf840e`)
- → ObservationPackage::len (`b4e4cbd9-d9f3-5d02-a4dc-0c11bdbedc05`)

### Contains

- ← ekos/crates/observation-sdk/src/lib.rs (`66ce958b-7250-5ec2-954e-eacf8f64aae0`)

## Diagram

```mermaid
graph TD
    nec52b091354f563d907203c8ef3092bf["source_fingerprint"]
    n66ce958b72505ec2954eeacf8f64aae0["ekos/crates/observation-sdk/src/lib.rs"]
    n66ce958b72505ec2954eeacf8f64aae0 -->|Contains| nec52b091354f563d907203c8ef3092bf
    n8b666d113ab852b18b717cb270682a0b["ScanContext::is_ignored"]
    nec52b091354f563d907203c8ef3092bf -->|Calls| n8b666d113ab852b18b717cb270682a0b
    ndd106f4a4759564d835a4d25afcf840e["ObservationPackage::push"]
    nec52b091354f563d907203c8ef3092bf -->|Calls| ndd106f4a4759564d835a4d25afcf840e
    nb4e4cbd9d9f35d02a4dc0c11bdbedc05["ObservationPackage::len"]
    nec52b091354f563d907203c8ef3092bf -->|Calls| nb4e4cbd9d9f35d02a4dc0c11bdbedc05
```

## Evidence

_No evidence cited._
