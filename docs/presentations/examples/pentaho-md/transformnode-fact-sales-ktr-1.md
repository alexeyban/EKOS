# fact_sales.ktr:1 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Calculate Sales Profit</name>
    <type>Formula</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <formula>
      <field_name>calculated_sales_profit</field_name>
      <formula_string>([UnitPrice] - [StandardCost]) * [OrderQty]</formula_string>
      <value_type>Number</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>608</xloc>
      <yloc>208</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: Formula |

## Relationships

### FeedsInto

- → fact_sales.ktr:15 (`7a2fa30b-5744-577e-af2c-f32b427aeb22`)
- ← fact_sales.ktr:3 (`699832f8-de69-55f9-966f-acf7612b60b1`)

## Diagram

```mermaid
graph TD
    n8a7a52f75eb65d4899cb406921ca48f6["fact_sales.ktr:1"]
    n7a2fa30b5744577eaf2cf32b427aeb22["fact_sales.ktr:15"]
    n8a7a52f75eb65d4899cb406921ca48f6 -->|FeedsInto| n7a2fa30b5744577eaf2cf32b427aeb22
    n699832f8de6955f9966facf7612b60b1["fact_sales.ktr:3"]
    n699832f8de6955f9966facf7612b60b1 -->|FeedsInto| n8a7a52f75eb65d4899cb406921ca48f6
```

## Evidence

- `2a5f5c89-fb74-5632-9cd8-885250c86994` — <step>
    <name>Calculate Sales Profit</name>
    <type>Formula</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <formula>
      <field_name>calculated_sales_profit</field_name>
      <formula_string>([UnitPrice] - [StandardCost]) * [OrderQty]</formula_string>
      <value_type>Number</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>608</xloc>
      <yloc>208</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
