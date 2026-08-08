# Architecture

## Components

- **Document**: 5
- **File**: 38
- **Person**: 1
- **PythonModule**: 7
- **PythonSymbol**: 13
- **Section**: 68
- **Technology**: 1

## Technologies

- **PostgreSQL** — used by: _no linked files_

## Entity Relationships

_No table foreign-key relationships compiled._

## Dependency Graph

### Contains

_81 `Contains` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### CoupledWith

_26 `CoupledWith` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### DependsOn

```mermaid
graph TD
    ndba2b0ac82015f99b4cdf4aed1647a6b["unknown"]
    n970f26267a695d358ca586ff8b31cd4b["PostgreSQL"]
    ndba2b0ac82015f99b4cdf4aed1647a6b -->|DependsOn| n970f26267a695d358ca586ff8b31cd4b
    nedb7dd60dbd0554c94b1d520028d1862["scripts/pg_inspect.py"]
    n939830d7a98154d5894ee35bb6220a55["argparse"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n939830d7a98154d5894ee35bb6220a55
    n1d419fa3da60585fbd0d182f82c95eaf["csv"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n1d419fa3da60585fbd0d182f82c95eaf
    n4330d5b3c9dd54eba4bcb5db46824aed["json"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n4330d5b3c9dd54eba4bcb5db46824aed
    n53c5b0d9d9a653f0b298ac43e5a17a62["os"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n53c5b0d9d9a653f0b298ac43e5a17a62
    n3f78f8ede81f52baaa5a3339f22ec469["subprocess"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n3f78f8ede81f52baaa5a3339f22ec469
    n73a609fa02435a5cbe38bc73b816eccb["sys"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n73a609fa02435a5cbe38bc73b816eccb
    n48707c04a7795f83a8ab62c998008561["io"]
    nedb7dd60dbd0554c94b1d520028d1862 -->|DependsOn| n48707c04a7795f83a8ab62c998008561
```

### OwnedBy

_43 `OwnedBy` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

