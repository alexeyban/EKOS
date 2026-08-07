# fact_sales.ktr:17 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Customer Lookup</name>
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
    <table>dim_customer</table>
    <connection>LocalMySQL-dataMart</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>CustomerID</name>
        <lookup>customer_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_customer_id</name>
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
      <xloc>400</xloc>
      <yloc>368</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: DimensionLookup |

## Relationships

### FeedsInto

- → fact_sales.ktr:9 (`71039590-7ed6-5761-a7c7-95fe29d56665`)
- ← fact_sales.ktr:16 (`2e91079f-fc19-57bc-9bc1-cf166fd9d16f`)

## Diagram

```mermaid
graph TD
    n554cd6c2c25d58d5878d13b2dd0210ab["fact_sales.ktr:17"]
    n710395907ed65761a7c795fe29d56665["fact_sales.ktr:9"]
    n554cd6c2c25d58d5878d13b2dd0210ab -->|FeedsInto| n710395907ed65761a7c795fe29d56665
    n2e91079ffc1957bc9bc1cf166fd9d16f["fact_sales.ktr:16"]
    n2e91079ffc1957bc9bc1cf166fd9d16f -->|FeedsInto| n554cd6c2c25d58d5878d13b2dd0210ab
```

## Evidence

- `ea9c54a2-c073-5401-b515-87e621e0532a` — <step>
    <name>Customer Lookup</name>
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
    <table>dim_customer</table>
    <connection>LocalMySQL-dataMart</connection>
    <commit>100</commit>
    <update>N</update>
    <fields>
      <key>
        <name>CustomerID</name>
        <lookup>customer_id</lookup>
      </key>
      <date>
        <name>OrderDate</name>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <return>
        <name>dim_customer_id</name>
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
      <xloc>400</xloc>
      <yloc>368</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
