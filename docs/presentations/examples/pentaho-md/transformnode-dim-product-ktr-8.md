# dim_product.ktr:8 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>dim_product_id</name>
    <type>Sequence</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <valuename>dim_product_id</valuename>
    <use_database>N</use_database>
    <connection/>
    <schema/>
    <seqname>SEQ_</seqname>
    <use_counter>Y</use_counter>
    <counter_name>dim_product</counter_name>
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
      <xloc>544</xloc>
      <yloc>128</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: Sequence |

## Relationships

### FeedsInto

- → dim_product.ktr:1 (`49ed182a-a70d-552c-aaf6-d746af182b4f`)
- ← dim_product.ktr:2 (`1ff1a6fd-c5d3-52a5-9df0-4395359ad6cd`)

## Diagram

```mermaid
graph TD
    n44d97c2a02425444ad469d9c6217899c["dim_product.ktr:8"]
    n49ed182aa70d552caaf6d746af182b4f["dim_product.ktr:1"]
    n44d97c2a02425444ad469d9c6217899c -->|FeedsInto| n49ed182aa70d552caaf6d746af182b4f
    n1ff1a6fdc5d352a59df04395359ad6cd["dim_product.ktr:2"]
    n1ff1a6fdc5d352a59df04395359ad6cd -->|FeedsInto| n44d97c2a02425444ad469d9c6217899c
```

## Evidence

- `86495456-20cd-551e-a70d-52114b90b358` — <step>
    <name>dim_product_id</name>
    <type>Sequence</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <valuename>dim_product_id</valuename>
    <use_database>N</use_database>
    <connection/>
    <schema/>
    <seqname>SEQ_</seqname>
    <use_counter>Y</use_counter>
    <counter_name>dim_product</counter_name>
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
      <xloc>544</xloc>
      <yloc>128</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
