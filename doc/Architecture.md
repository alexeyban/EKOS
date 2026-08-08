# Architecture

## Components

- **Document**: 13
- **File**: 751
- **Person**: 2
- **PythonModule**: 3
- **PythonSymbol**: 3
- **RustModule**: 441
- **RustSymbol**: 1301
- **Section**: 1518
- **Table**: 19
- **TransformNode**: 34

## Technologies

_No technology dependencies compiled._

## Entity Relationships

```mermaid
erDiagram
    "Employees" }o--|| "Employees" : references
    "Orders" }o--|| "Customers" : references
    "Orders" }o--|| "Employees" : references
    "Orders" }o--|| "Shippers" : references
    "Products" }o--|| "Categories" : references
    "Products" }o--|| "Suppliers" : references
    "'Order Details'" }o--|| "Orders" : references
    "'Order Details'" }o--|| "Products" : references
    "Territories" }o--|| "Region" : references
    "EmployeeTerritories" }o--|| "Employees" : references
    "EmployeeTerritories" }o--|| "Territories" : references
    "CustomerCustomerDemo" }o--|| "Customers" : references
    "CustomerCustomerDemo" }o--|| "CustomerDemographics" : references
    "categories" }o--|| "categories" : references
    "products" }o--|| "categories" : references
    "orders" }o--|| "customers" : references
    "order_items" }o--|| "orders" : references
    "order_items" }o--|| "products" : references
    "payments" }o--|| "orders" : references
```

## Dependency Graph

### Calls

_733 `Calls` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### Contains

_2822 `Contains` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### CoupledWith

_372 `CoupledWith` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### DependsOn

_1287 `DependsOn` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

### ForeignKey

```mermaid
graph TD
    n18ddc966658845b18a845a6ff92a9460["Employees"]
    n18ddc966658845b18a845a6ff92a9460 -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    ncd8c2d9d78e5430daf8816be08817b86["Orders"]
    ne0c56e7596774432b53e177fb7ecbad3["Customers"]
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| ne0c56e7596774432b53e177fb7ecbad3
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    nb40ad1cbc0184fbb8cdc0ec91049655f["Shippers"]
    ncd8c2d9d78e5430daf8816be08817b86 -->|ForeignKey| nb40ad1cbc0184fbb8cdc0ec91049655f
    n42110141a81c441a97c1094420890bed["Products"]
    ncb4c8bc084294ee9b5778dfe6a4bedb7["Categories"]
    n42110141a81c441a97c1094420890bed -->|ForeignKey| ncb4c8bc084294ee9b5778dfe6a4bedb7
    nd7162ea30ab240b6b4e3a4d788138303["Suppliers"]
    n42110141a81c441a97c1094420890bed -->|ForeignKey| nd7162ea30ab240b6b4e3a4d788138303
    naad094bf64d5428b99a64192cef78a08["'Order Details'"]
    naad094bf64d5428b99a64192cef78a08 -->|ForeignKey| ncd8c2d9d78e5430daf8816be08817b86
    naad094bf64d5428b99a64192cef78a08 -->|ForeignKey| n42110141a81c441a97c1094420890bed
    nf1d6c69d10954e1ba7dbca8a1fd33179["Territories"]
    na98521ec744f4127b06f59f94062bf33["Region"]
    nf1d6c69d10954e1ba7dbca8a1fd33179 -->|ForeignKey| na98521ec744f4127b06f59f94062bf33
    nad1c159d481547e6993863834ea6bd1a["EmployeeTerritories"]
    nad1c159d481547e6993863834ea6bd1a -->|ForeignKey| n18ddc966658845b18a845a6ff92a9460
    nad1c159d481547e6993863834ea6bd1a -->|ForeignKey| nf1d6c69d10954e1ba7dbca8a1fd33179
    n06b01f7296b14a439e148b0abb2b8d14["CustomerCustomerDemo"]
    n06b01f7296b14a439e148b0abb2b8d14 -->|ForeignKey| ne0c56e7596774432b53e177fb7ecbad3
    nd3d7c029bf964160bb9c732b5f01fe2d["CustomerDemographics"]
    n06b01f7296b14a439e148b0abb2b8d14 -->|ForeignKey| nd3d7c029bf964160bb9c732b5f01fe2d
    n6794f52865834e8d89842358f199ce12["categories"]
    n6794f52865834e8d89842358f199ce12 -->|ForeignKey| n6794f52865834e8d89842358f199ce12
    n7c2e13d4535e435d992e0db2d71e2c6d["products"]
    n7c2e13d4535e435d992e0db2d71e2c6d -->|ForeignKey| n6794f52865834e8d89842358f199ce12
    nad94b10656104be3905ecf114df00129["orders"]
    ndc844ed6954d4cca8ae89597873eb56e["customers"]
    nad94b10656104be3905ecf114df00129 -->|ForeignKey| ndc844ed6954d4cca8ae89597873eb56e
    n08a0316015ed4f15bbc297c816faf313["order_items"]
    n08a0316015ed4f15bbc297c816faf313 -->|ForeignKey| nad94b10656104be3905ecf114df00129
    n08a0316015ed4f15bbc297c816faf313 -->|ForeignKey| n7c2e13d4535e435d992e0db2d71e2c6d
    nc663212aa5f94534a3a5a47fa4f31a9b["payments"]
    nc663212aa5f94534a3a5a47fa4f31a9b -->|ForeignKey| nad94b10656104be3905ecf114df00129
```

### OwnedBy

_102 `OwnedBy` relationships compiled — diagram omitted, too large to render usefully. See `ekos docs generate --layout objects` for per-object detail._

