select
    *
from {{ source('pentaho', 'Sales.SalesPerson') }}
