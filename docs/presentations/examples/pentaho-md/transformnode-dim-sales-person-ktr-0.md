# dim_sales_person.ktr:0 (TransformNode)

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
    <valuename>dim_sales_person_id</valuename>
    <use_database>N</use_database>
    <connection>SqlServer</connection>
    <schema>Sales</schema>
    <seqname>ID</seqname>
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
      <xloc>240</xloc>
      <yloc>160</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: Sequence |

## Relationships

### FeedsInto

- → dim_sales_person.ktr:2 (`e2f19a19-f1ce-55d6-bdfa-c60c9cd35e5c`)
- ← dim_sales_person.ktr:6 (`676e9ff6-3347-59ed-8927-30e1dee7aafe`)

## Diagram

```mermaid
graph TD
    nd3d9a08da35351f5894591da2a42146d["dim_sales_person.ktr:0"]
    ne2f19a19f1ce55d6bdfac60c9cd35e5c["dim_sales_person.ktr:2"]
    nd3d9a08da35351f5894591da2a42146d -->|FeedsInto| ne2f19a19f1ce55d6bdfac60c9cd35e5c
    n676e9ff6334759ed892730e1dee7aafe["dim_sales_person.ktr:6"]
    n676e9ff6334759ed892730e1dee7aafe -->|FeedsInto| nd3d9a08da35351f5894591da2a42146d
```

## Evidence

- `3b98402d-2268-52ef-b737-0c5be5f381f3` — <step>
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
    <valuename>dim_sales_person_id</valuename>
    <use_database>N</use_database>
    <connection>SqlServer</connection>
    <schema>Sales</schema>
    <seqname>ID</seqname>
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
      <xloc>240</xloc>
      <yloc>160</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
