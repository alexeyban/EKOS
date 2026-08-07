# fact_purchase.ktr:7 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Product Lookup</name>
    <type>DimensionLookup</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <schema>eae_data_management_mmjja</schema>
    <table>dim_product</table>
    <connection>mySql_eae_Dm</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>ProductID</name>
        <lookup>product_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_product_id</name>
        <rename/>
        <creation_method>tablemax</creation_method>
        <use_autoinc>N</use_autoinc>
        <version>version_number</version>
      </return>
    </fields>
    <sequence/>
    <min_year>1900</min_year>
    <max_year>2199</max_year>
    <cache_size>5000</cache_size>
    <preload_cache>N</preload_cache>
    <use_start_date_alternative>N</use_start_date_alternative>
    <start_date_alternative>none</start_date_alternative>
    <start_date_field_name/>
    <useBatch>N</useBatch>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>384</xloc>
      <yloc>208</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: DimensionLookup |

## Relationships

### FeedsInto

- → fact_purchase.ktr:8 (`9ecd6f0e-b556-5b18-9afc-e9fdabc61e95`)
- ← fact_purchase.ktr:0 (`8ef59248-d2ee-5fee-a746-e41ca8eeee69`)

## Diagram

```mermaid
graph TD
    naacf805dac0d5ebe8eb5be7f9dea1724["fact_purchase.ktr:7"]
    n9ecd6f0eb5565b189afce9fdabc61e95["fact_purchase.ktr:8"]
    naacf805dac0d5ebe8eb5be7f9dea1724 -->|FeedsInto| n9ecd6f0eb5565b189afce9fdabc61e95
    n8ef59248d2ee5feea746e41ca8eeee69["fact_purchase.ktr:0"]
    n8ef59248d2ee5feea746e41ca8eeee69 -->|FeedsInto| naacf805dac0d5ebe8eb5be7f9dea1724
```

## Evidence

- `804ae00e-e61d-59e2-a20a-99f39f589d0d` — <step>
    <name>Product Lookup</name>
    <type>DimensionLookup</type>
    <description/>
    <distribute>Y</distribute>
    <custom_distribution/>
    <copies>1</copies>
    <partitioning>
      <method>none</method>
      <schema_name/>
    </partitioning>
    <schema>eae_data_management_mmjja</schema>
    <table>dim_product</table>
    <connection>mySql_eae_Dm</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>ProductID</name>
        <lookup>product_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_product_id</name>
        <rename/>
        <creation_method>tablemax</creation_method>
        <use_autoinc>N</use_autoinc>
        <version>version_number</version>
      </return>
    </fields>
    <sequence/>
    <min_year>1900</min_year>
    <max_year>2199</max_year>
    <cache_size>5000</cache_size>
    <preload_cache>N</preload_cache>
    <use_start_date_alternative>N</use_start_date_alternative>
    <start_date_alternative>none</start_date_alternative>
    <start_date_field_name/>
    <useBatch>N</useBatch>
    <attributes/>
    <cluster_schema/>
    <remotesteps>
      <input>
      </input>
      <output>
      </output>
    </remotesteps>
    <GUI>
      <xloc>384</xloc>
      <yloc>208</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
