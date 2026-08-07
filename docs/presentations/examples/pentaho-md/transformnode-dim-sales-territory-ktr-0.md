# dim_sales_territory.ktr:0 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Add sequence</name>
    <type>Sequence</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <valuename>Sales_territory_ID</valuename>
    <use_database>N</use_database>
    <connection/>
    <schema/>
    <seqname>SEQ_</seqname>
    <use_counter>Y</use_counter>
    <counter_name/>
    <start_at>1</start_at>
    <increment_by>1</increment_by>
    <max_value>999999999</max_value>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>288</xloc>
      <yloc>144</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: Sequence |

## Relationships

### FeedsInto

- → dim_sales_territory.ktr:2 (`75d273d0-c02b-5ad5-accb-deef5c5ea5d8`)
- ← dim_sales_territory.ktr:1 (`e75f47f9-2efd-5225-b8a9-6d4e3559885c`)

## Diagram

```mermaid
graph TD
    n8be5eef98a945ef1a9a71e66787f8c4c["dim_sales_territory.ktr:0"]
    n75d273d0c02b5ad5accbdeef5c5ea5d8["dim_sales_territory.ktr:2"]
    n8be5eef98a945ef1a9a71e66787f8c4c -->|FeedsInto| n75d273d0c02b5ad5accbdeef5c5ea5d8
    ne75f47f92efd5225b8a96d4e3559885c["dim_sales_territory.ktr:1"]
    ne75f47f92efd5225b8a96d4e3559885c -->|FeedsInto| n8be5eef98a945ef1a9a71e66787f8c4c
```

## Evidence

- `8cdc8cb4-c3d0-59e3-a590-a26b316761c0` — <step>
    <name>Add sequence</name>
    <type>Sequence</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <valuename>Sales_territory_ID</valuename>
    <use_database>N</use_database>
    <connection/>
    <schema/>
    <seqname>SEQ_</seqname>
    <use_counter>Y</use_counter>
    <counter_name/>
    <start_at>1</start_at>
    <increment_by>1</increment_by>
    <max_value>999999999</max_value>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>288</xloc>
      <yloc>144</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
