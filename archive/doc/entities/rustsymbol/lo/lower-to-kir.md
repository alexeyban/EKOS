# lower_to_kir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → transform_evidence_kir_id (`acdcb5e6-413e-57b0-bb0b-d519a247e071`)
- → transform_node_kir_id (`3b4e5098-42ef-5a21-894d-ac9ebe86762a`)
- → TransformNode::node_type (`d4c1c00f-b050-5363-b81a-6b984bd7812f`)
- → TransformNode::evidence_fragment (`98f701d7-665e-5625-87e1-27a9028c2efe`)
- → TransformNode::properties (`2e58103d-045e-5409-bf85-e4e0a21186c3`)

### Contains

- ← ekos/crates/semantic/src/transform_ir.rs (`b4fdd24c-8184-5879-9136-f0a70208955e`)

## Diagram

```mermaid
graph TD
    n937c6e2defa65b74bfcb3bee881ee4ab["lower_to_kir"]
    nb4fdd24c818458799136f0a70208955e["ekos/crates/semantic/src/transform_ir.rs"]
    nb4fdd24c818458799136f0a70208955e -->|Contains| n937c6e2defa65b74bfcb3bee881ee4ab
    nacdcb5e6413e57b0bb0bd519a247e071["transform_evidence_kir_id"]
    n937c6e2defa65b74bfcb3bee881ee4ab -->|Calls| nacdcb5e6413e57b0bb0bd519a247e071
    n3b4e509842ef5a21894dac9ebe86762a["transform_node_kir_id"]
    n937c6e2defa65b74bfcb3bee881ee4ab -->|Calls| n3b4e509842ef5a21894dac9ebe86762a
    nd4c1c00fb0505363b81a6b984bd7812f["TransformNode::node_type"]
    n937c6e2defa65b74bfcb3bee881ee4ab -->|Calls| nd4c1c00fb0505363b81a6b984bd7812f
    n98f701d7665e562587e127a9028c2efe["TransformNode::evidence_fragment"]
    n937c6e2defa65b74bfcb3bee881ee4ab -->|Calls| n98f701d7665e562587e127a9028c2efe
    n2e58103d045e5409bf85e4e0a21186c3["TransformNode::properties"]
    n937c6e2defa65b74bfcb3bee881ee4ab -->|Calls| n2e58103d045e5409bf85e4e0a21186c3
```

## Evidence

_No evidence cited._
