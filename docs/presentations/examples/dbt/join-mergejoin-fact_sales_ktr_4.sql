-- NOTE: l/r assignment is positional (first two FeedsInto edges), not guaranteed to match the original Kettle step's left/right
select *
from {{ ref('fact_sales_ktr_12') }} as l
inner join {{ ref('fact_sales_ktr_11') }} as r
    on true -- no join keys compiled
