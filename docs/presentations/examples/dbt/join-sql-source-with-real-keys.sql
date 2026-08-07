-- NOTE: l/r assignment is positional (first two FeedsInto edges), not guaranteed to match the original Kettle step's left/right
select *
from {{ ref('db_scripts_destination_mysql_datamart_testing_scenarios_sql_2_0') }} as l
inner join {{ ref('db_scripts_destination_mysql_datamart_testing_scenarios_sql_2_1') }} as r
    on s.dim_product_id = p.dim_product_id -- TODO: verify column qualification, source dialect: Pentaho
