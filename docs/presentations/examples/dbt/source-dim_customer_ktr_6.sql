select
    BusinessEntityID,
    TerritoryID
from {{ source('pentaho', 'Sales.SalesPerson') }}
