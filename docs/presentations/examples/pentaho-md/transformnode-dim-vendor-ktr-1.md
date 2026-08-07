# dim_vendor.ktr:1 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Dimension lookup/update</name>
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
    <connection>MySql datamart</connection>
    <commit>100</commit>
    <update>Y</update>
    <fields>
      <key>
        <name>BusinessEntityID</name>
        <lookup>vendor_id</lookup>
      </key>
      <date>
        <name/>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <field>
        <name>PreferredVendorStatus</name>
        <lookup>preferred_vendor</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>AccountNumber</name>
        <lookup>account_number</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>Name</name>
        <lookup>name</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>CreditRating</name>
        <lookup>credit_rating_id</lookup>
        <update>Insert</update>
      </field>
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
    <cache_size>-1</cache_size>
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
      <xloc>640</xloc>
      <yloc>64</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: DimensionLookup |

## Relationships

### FeedsInto

- ← dim_vendor.ktr:0 (`b5c7d07e-3cd7-561b-a5a5-d363ef97f6c7`)

## Diagram

```mermaid
graph TD
    na109b5dabf9f55e38d784669fc7745f9["dim_vendor.ktr:1"]
    nb5c7d07e3cd7561ba5a5d363ef97f6c7["dim_vendor.ktr:0"]
    nb5c7d07e3cd7561ba5a5d363ef97f6c7 -->|FeedsInto| na109b5dabf9f55e38d784669fc7745f9
```

## Evidence

- `be9267a6-a85b-5cef-a2c2-c32beaa22be6` — <step>
    <name>Dimension lookup/update</name>
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
    <connection>MySql datamart</connection>
    <commit>100</commit>
    <update>Y</update>
    <fields>
      <key>
        <name>BusinessEntityID</name>
        <lookup>vendor_id</lookup>
      </key>
      <date>
        <name/>
        <from>start_date</from>
        <to>end_date</to>
      </date>
      <field>
        <name>PreferredVendorStatus</name>
        <lookup>preferred_vendor</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>AccountNumber</name>
        <lookup>account_number</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>Name</name>
        <lookup>name</lookup>
        <update>Insert</update>
      </field>
      <field>
        <name>CreditRating</name>
        <lookup>credit_rating_id</lookup>
        <update>Insert</update>
      </field>
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
    <cache_size>-1</cache_size>
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
      <xloc>640</xloc>
      <yloc>64</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
