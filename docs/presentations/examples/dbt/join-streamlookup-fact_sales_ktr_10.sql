-- NOTE: l/r assignment is positional (first two FeedsInto edges), not guaranteed to match the original Kettle step's left/right
select *
from {{ ref('fact_sales_ktr_0') }} as l
left join {{ ref('fact_sales_ktr_14') }} as r
    on true -- no join keys compiled
