# fact_purchase.ktr:8 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Vendor Lookup</name>
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
    <table>dim_vendor</table>
    <connection>mySql_eae_Dm</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>VendorID</name>
        <lookup>vendor_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_vendor_id</name>
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
      <xloc>464</xloc>
      <yloc>304</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: DimensionLookup |

## Relationships

### FeedsInto

- → fact_purchase.ktr:1 (`bc5e6ac3-c306-5ec8-850f-c5ac0d6afd57`)
- ← fact_purchase.ktr:7 (`aacf805d-ac0d-5ebe-8eb5-be7f9dea1724`)

## Diagram

```mermaid
graph TD
    n9ecd6f0eb5565b189afce9fdabc61e95["fact_purchase.ktr:8"]
    nbc5e6ac3c3065ec8850fc5ac0d6afd57["fact_purchase.ktr:1"]
    n9ecd6f0eb5565b189afce9fdabc61e95 -->|FeedsInto| nbc5e6ac3c3065ec8850fc5ac0d6afd57
    naacf805dac0d5ebe8eb5be7f9dea1724["fact_purchase.ktr:7"]
    naacf805dac0d5ebe8eb5be7f9dea1724 -->|FeedsInto| n9ecd6f0eb5565b189afce9fdabc61e95
```

## Evidence

- `df1b3a8f-716a-5732-b724-7c4fb53f761f` — <step>
    <name>Vendor Lookup</name>
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
    <table>dim_vendor</table>
    <connection>mySql_eae_Dm</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>VendorID</name>
        <lookup>vendor_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_vendor_id</name>
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
      <xloc>464</xloc>
      <yloc>304</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
