# fact_sales.ktr:0 (TransformNode)

## Properties

| Key | Value |
|---|---|
| `node_type` | Unmapped |
| `raw` | <step>
    <name>Calculate Date Ids and Amounts</name>
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
      <field_name>calculated_order_date_id</field_name>
      <formula_string>[order_date_year]*10000 + [order_date_month]*100 + [order_date_day]</formula_string>
      <value_type>Integer</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <formula>
      <field_name>calculated_ship_date_id</field_name>
      <formula_string>[ship_date_year]*10000 + [ship_date_month]*100 + [ship_date_day]</formula_string>
      <value_type>Integer</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <formula>
      <field_name>calculated_discount_amount</field_name>
      <formula_string>[UnitPrice] * [UnitPriceDiscount] * [OrderQty]</formula_string>
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
      <xloc>400</xloc>
      <yloc>96</yloc>
      <draw>Y</draw>
    </GUI>
  </step> |
| `reason` | unrecognized step type: Formula |

## Relationships

### FeedsInto

- → fact_sales.ktr:10 (`0b3339b9-475a-55b0-bb7e-cb7e7c34fa8c`)
- ← fact_sales.ktr:2 (`0c1ebe1c-8159-5483-9e8d-8a04c7c91a2e`)

## Diagram

```mermaid
graph TD
    nbd436d40405357f4ac77b4ffe37edae4["fact_sales.ktr:0"]
    n0b3339b9475a55b0bb7ecb7e7c34fa8c["fact_sales.ktr:10"]
    nbd436d40405357f4ac77b4ffe37edae4 -->|FeedsInto| n0b3339b9475a55b0bb7ecb7e7c34fa8c
    n0c1ebe1c815954839e8d8a04c7c91a2e["fact_sales.ktr:2"]
    n0c1ebe1c815954839e8d8a04c7c91a2e -->|FeedsInto| nbd436d40405357f4ac77b4ffe37edae4
```

## Evidence

- `1ab7fa44-0462-57c4-8dff-fb1a96159baa` — <step>
    <name>Calculate Date Ids and Amounts</name>
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
      <field_name>calculated_order_date_id</field_name>
      <formula_string>[order_date_year]*10000 + [order_date_month]*100 + [order_date_day]</formula_string>
      <value_type>Integer</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <formula>
      <field_name>calculated_ship_date_id</field_name>
      <formula_string>[ship_date_year]*10000 + [ship_date_month]*100 + [ship_date_day]</formula_string>
      <value_type>Integer</value_type>
      <value_length>-1</value_length>
      <value_precision>-1</value_precision>
      <replace_field/>
    </formula>
    <formula>
      <field_name>calculated_discount_amount</field_name>
      <formula_string>[UnitPrice] * [UnitPriceDiscount] * [OrderQty]</formula_string>
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
      <xloc>400</xloc>
      <yloc>96</yloc>
      <draw>Y</draw>
    </GUI>
  </step> (confidence: 1.00)
