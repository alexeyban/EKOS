# fact_sales.ktr:12 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Sort ProductId</name>
    <type>SortRows</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <directory>%%java.io.tmpdir%%</directory>
    <prefix>out</prefix>
    <sort_size>1000000</sort_size>
    <free_memory/>
    <compress>N</compress>
    <compress_variable/>
    <unique_rows>N</unique_rows>
    <fields>
      <field>
        <name>ProductID</name>
        <ascending>Y</ascending>
        <case_sensitive>N</case_sensitive>
        <collator_enabled>N</collator_enabled>
        <collator_strength>0</collator_strength>
        <presorted>N</presorted>
      </field>
    </fields>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>528</xloc>
      <yloc>496</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: SortRows |

## Relationships

### FeedsInto

- → fact_sales.ktr:4 (`8eb94913-b4b9-5d88-915d-7fb890edd830`)
- ← fact_sales.ktr:9 (`71039590-7ed6-5761-a7c7-95fe29d56665`)

## Diagram

```mermaid
graph TD
    ne54591c9e1d958808ebaf77196e45271["fact_sales.ktr:12"]
    n8eb94913b4b95d88915d7fb890edd830["fact_sales.ktr:4"]
    ne54591c9e1d958808ebaf77196e45271 -->|FeedsInto| n8eb94913b4b95d88915d7fb890edd830
    n710395907ed65761a7c795fe29d56665["fact_sales.ktr:9"]
    n710395907ed65761a7c795fe29d56665 -->|FeedsInto| ne54591c9e1d958808ebaf77196e45271
```

## Evidence

- `03a7da11-daa6-52d2-b629-350055286671` — <step>
    <name>Sort ProductId</name>
    <type>SortRows</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <directory>%%java.io.tmpdir%%</directory>
    <prefix>out</prefix>
    <sort_size>1000000</sort_size>
    <free_memory/>
    <compress>N</compress>
    <compress_variable/>
    <unique_rows>N</unique_rows>
    <fields>
      <field>
        <name>ProductID</name>
        <ascending>Y</ascending>
        <case_sensitive>N</case_sensitive>
        <collator_enabled>N</collator_enabled>
        <collator_strength>0</collator_strength>
        <presorted>N</presorted>
      </field>
    </fields>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>528</xloc>
      <yloc>496</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
