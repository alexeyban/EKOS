from __future__ import annotations

import re
import sys
from datetime import (
    date,
    datetime,
    time
)
from decimal import Decimal
from enum import Enum
from typing import (
    Any,
    ClassVar,
    Literal,
    Optional,
    Union
)

from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    RootModel,
    SerializationInfo,
    SerializerFunctionWrapHandler,
    field_validator,
    model_serializer
)


metamodel_version = "1.11.0"
version = "None"


class ConfiguredBaseModel(BaseModel):
    model_config = ConfigDict(
        serialize_by_alias = True,
        validate_by_name = True,
        validate_assignment = True,
        validate_default = True,
        extra = "forbid",
        arbitrary_types_allowed = True,
        use_enum_values = True,
        strict = False,
    )





class LinkMLMeta(RootModel):
    root: dict[str, Any] = {}
    model_config = ConfigDict(frozen=True)

    def __getattr__(self, key:str):
        return getattr(self.root, key)

    def __getitem__(self, key:str):
        return self.root[key]

    def __setitem__(self, key:str, value):
        self.root[key] = value

    def __contains__(self, key:str) -> bool:
        return key in self.root


linkml_meta = LinkMLMeta({'annotations': {'ekos_export_status': {'tag': 'ekos_export_status',
                                            'value': 'confirmed'}},
     'default_prefix': 'ledgersmb',
     'default_range': 'string',
     'description': 'Draft business-semantics schema recovered by EKOS (RFC 0170) '
                    'from SQL predicates, CHECK constraints, lookup seed rows, '
                    'column comments and git history. Classes derived from tables '
                    'are observed structure; concepts and enum meanings are '
                    'hypotheses until a domain expert confirms them.',
     'id': 'https://w3id.org/ekos/ledgersmb',
     'imports': ['linkml:types'],
     'name': 'ledgersmb',
     'prefixes': {'ekos': {'prefix_prefix': 'ekos',
                           'prefix_reference': 'https://w3id.org/ekos/'},
                  'ledgersmb': {'prefix_prefix': 'ledgersmb',
                                'prefix_reference': 'https://w3id.org/ekos/ledgersmb/'},
                  'linkml': {'prefix_prefix': 'linkml',
                             'prefix_reference': 'https://w3id.org/linkml/'}},
     'source_file': '/home/legion/PycharmProjects/EKOS/demo/ledgersmb-linkml/work/ledgersmb.linkml.yaml',
     'title': 'ledgersmb — business semantics recovered by EKOS (draft)'} )

class AccountCategory(str, Enum):
    """
    Coded values of `account.category` recovered from code traces.
    """
    A = "A"
    """
    Asset
    """
    E = "E"
    """
    Expense
    """
    I = "I"
    """
    Income
    """
    L = "L"
    """
    Liability
    """
    Q = "Q"
    """
    Equity
    """


class EntityCreditAccountEntityClass(str, Enum):
    """
    Coded values of `entity_credit_account.entity_class` recovered from code traces.
    """
    number_1 = "1"
    """
    Vendor
    """
    number_2 = "2"
    """
    Customer
    """
    number_3 = "3"
    """
    Employee
    """


class OeOeClassId(str, Enum):
    """
    Coded values of `oe.oe_class_id` recovered from code traces.
    """
    number_1 = "1"
    """
    Sales Order
    """
    number_2 = "2"
    """
    Purchase Order
    """
    number_3 = "3"
    """
    Quotation
    """
    number_4 = "4"
    """
    RFQ
    """



class Account(ConfiguredBaseModel):
    """
    This table stores the main account info.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_table': {'tag': 'ekos_table', 'value': 'account'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb',
         'slot_usage': {'category': {'annotations': {'ekos_constraint': {'tag': 'ekos_constraint',
                                                                         'value': 'category '
                                                                                  'IN '
                                                                                  "('A', "
                                                                                  "'L', "
                                                                                  "'Q', "
                                                                                  "'I', "
                                                                                  "'E')"},
                                                     'ekos_evidence': {'tag': 'ekos_evidence',
                                                                       'value': 'sql/Pg-database.sql:64'},
                                                     'ekos_id': {'tag': 'ekos_id',
                                                                 'value': '04dfd199-5b22-5b03-81cd-e5037d87a663'},
                                                     'ekos_status': {'tag': 'ekos_status',
                                                                     'value': 'confirmed'}},
                                     'name': 'category',
                                     'range': 'AccountCategory'}}})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    accno: str = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    is_temp: bool = Field(default=..., description="""Only affects equity accounts.  If set, close at end of year.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account']} })
    category: AccountCategory = Field(default=..., description="""A=asset,L=liability,Q=Equity,I=Income,E=expense""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_constraint': {'tag': 'ekos_constraint',
                                             'value': "category IN ('A', 'L', 'Q', "
                                                      "'I', 'E')"},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/Pg-database.sql:64'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '04dfd199-5b22-5b03-81cd-e5037d87a663'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'domain_of': ['Account']} })
    gifi_accno: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account']} })
    heading: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Account']} })
    contra: bool = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account']} })
    tax: bool = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account']} })
    obsolete: bool = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account', 'Parts']} })


class EntityCreditAccount(ConfiguredBaseModel):
    """
    This table stores information relating to general relationships regarding
    moneys owed on invoice.  Invoices, whether AR or AP, must be attached to
    a record in this table.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_table': {'tag': 'ekos_table',
                                        'value': 'entity_credit_account'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    entity_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    entity_class: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_enum': {'tag': 'ekos_enum',
                                       'value': 'EntityCreditAccountEntityClass'},
                         'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    pay_to_name: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    discount_terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxincluded: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    creditlimit: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT2'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    meta_number: Optional[str] = Field(default=None, description="""This stores the human readable control code for the customer/vendor record.
This is typically called the customer/vendor \"account\" in the application.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(32)'}},
         'domain_of': ['EntityCreditAccount']} })
    business_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    language_code: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(6)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    pricegroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    curr: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'CHAR(3)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    startdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    enddate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    threshold: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    employee_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    primary_contact: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    ar_ap_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    cash_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    bank_account: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxform_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })


class Oe(ConfiguredBaseModel):
    """
    Header information for:
    * Sales orders
    * Purchase Orders
    * Quotations
    * Requests for Quotation
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_table': {'tag': 'ekos_table', 'value': 'oe'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    ordnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    transdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe', 'Yearend']} })
    entity_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INTEGER'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    amount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Oe']} })
    netamount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Oe']} })
    reqdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe']} })
    taxincluded: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    shippingpoint: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    curr: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'CHAR(3)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    person_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INTEGER'}},
         'domain_of': ['Oe']} })
    closed: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Oe']} })
    quotation: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Oe']} })
    quonumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    intnotes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    shipvia: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    language_code: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(6)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    ponumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT2'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    entity_credit_account: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Oe']} })
    oe_class_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_enum': {'tag': 'ekos_enum', 'value': 'OeOeClassId'},
                         'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Oe']} })


class Parts(ConfiguredBaseModel):
    """
    This stores detail information about goods and services.  The type of part
    is currently defined according to the following rules:
    * If assembly is true, then an assembly
    * If inventory_accno_id, income_accno_id, and expense_accno_id are not null then
      a part.
    * If inventory_accno_id is null but the other two are not, then a service.
    * Otherwise, a labor/overhead entry.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_suggested_close_mappings': {'tag': 'ekos_suggested_close_mappings',
                                                           'value': 'schema:Product'},
                         'ekos_table': {'tag': 'ekos_table', 'value': 'parts'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    partnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    unit: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(5)'}},
         'domain_of': ['Parts']} })
    listprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    sellprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    lastcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    priceupdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Parts']} })
    weight: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    onhand: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    makemodel: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    assembly: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    alternate: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    rop: Optional[Decimal] = Field(default=None, description="""Re-order point.  Used to select parts for short inventory report.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    inventory_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    income_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    expense_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    returns_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    bin: Optional[str] = Field(default=None, description="""Text identifier for where a part is stored.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    obsolete: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account', 'Parts']} })
    bom: Optional[bool] = Field(default=None, description="""Show on Bill of Materials.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    image: Optional[str] = Field(default=None, description="""Hyperlink to product image.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    drawing: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    microfiche: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    partsgroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    avgcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })


class Transactions(ConfiguredBaseModel):
    """
    This table provides referential integrity between AR, AP, GL tables on one
    hand and acc_trans on the other, pending the refactoring of those tables.  It
    also is used to provide discretionary locking of financial transactions across
    database connections, for example in batch payment workflows.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_table': {'tag': 'ekos_table', 'value': 'transactions'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    table_name: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Transactions']} })
    locked_by: Optional[int] = Field(default=None, description="""This should only be used in pessimistic locking measures as required by large
batch work flows.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Transactions']} })
    approved: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Transactions']} })
    approved_by: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Transactions']} })
    approved_at: Optional[datetime ] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'TIMESTAMP'}},
         'domain_of': ['Transactions']} })


class Yearend(ConfiguredBaseModel):
    """
    An extension to the journal_entry table to track transactionsactions which close out
    the books at yearend.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_status': {'tag': 'ekos_status', 'value': 'observed'},
                         'ekos_table': {'tag': 'ekos_table', 'value': 'yearend'}},
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    trans_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Yearend']} })
    reversed: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Yearend']} })
    transdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe', 'Yearend']} })


class CustomerAccount(EntityCreditAccount):
    """
    A credit account held by a customer (entity_class 2).
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.45'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'entity_credit_account.entity_class '
                                                      'IN (2)'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/Company.sql:1297; '
                                                    'sql/modules/Company.sql:1310; '
                                                    'sql/modules/Company.sql:1397'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '074fe04f-ed9b-548d-9201-6bc644be2ea6'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 1b5baf71b0 "Groundwork '
                                                     'for fix on pricematrix issues" '
                                                     '(2011-11-22); commit 439aaf1160 '
                                                     '"More customer/vendor '
                                                     'pricematrix work, again not '
                                                     'exposed to app, and not '
                                                     'affecting previous '
                                                     'functionality" (2011-11-23); '
                                                     'commit e9d0fbd521 "Company name '
                                                     'can now be prepopulated on login '
                                                     'screen" (2011-11-21)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 3 site(s); name given by '
                      'demo-reviewer (recovered as `EntityCreditAccountCustomer`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    entity_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    entity_class: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_enum': {'tag': 'ekos_enum',
                                       'value': 'EntityCreditAccountEntityClass'},
                         'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    pay_to_name: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    discount_terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxincluded: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    creditlimit: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT2'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    meta_number: Optional[str] = Field(default=None, description="""This stores the human readable control code for the customer/vendor record.
This is typically called the customer/vendor \"account\" in the application.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(32)'}},
         'domain_of': ['EntityCreditAccount']} })
    business_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    language_code: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(6)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    pricegroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    curr: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'CHAR(3)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    startdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    enddate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    threshold: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    employee_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    primary_contact: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    ar_ap_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    cash_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    bank_account: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxform_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })


class VendorAccount(EntityCreditAccount):
    """
    A credit account held by a vendor (entity_class 1).
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.45'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'entity_credit_account.entity_class '
                                                      'IN (1)'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/Company.sql:1318; '
                                                    'sql/modules/Company.sql:1368; '
                                                    'sql/modules/Util.sql:100'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '5b651f5b-f613-5836-827c-c533af341739'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 439aaf1160 "More '
                                                     'customer/vendor pricematrix '
                                                     'work, again not exposed to app, '
                                                     'and not affecting previous '
                                                     'functionality" (2011-11-23); '
                                                     'commit df1b059b1a "Merging '
                                                     'Util.sql from addons into main" '
                                                     '(2011-07-05); commit e9d0fbd521 '
                                                     '"Company name can now be '
                                                     'prepopulated on login screen" '
                                                     '(2011-11-21)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 3 site(s); name given by '
                      'demo-reviewer (recovered as `EntityCreditAccountVendor`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    entity_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    entity_class: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_enum': {'tag': 'ekos_enum',
                                       'value': 'EntityCreditAccountEntityClass'},
                         'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    pay_to_name: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    discount_terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    discount_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxincluded: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    creditlimit: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT2'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    meta_number: Optional[str] = Field(default=None, description="""This stores the human readable control code for the customer/vendor record.
This is typically called the customer/vendor \"account\" in the application.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(32)'}},
         'domain_of': ['EntityCreditAccount']} })
    business_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    language_code: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(6)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    pricegroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    curr: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'CHAR(3)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    startdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    enddate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['EntityCreditAccount']} })
    threshold: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['EntityCreditAccount']} })
    employee_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    primary_contact: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    ar_ap_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    cash_account_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    bank_account: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })
    taxform_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['EntityCreditAccount']} })


class OpenOrder(Oe):
    """
    An order or quotation not yet closed.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.4'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'oe.closed IS FALSE'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/Goods.sql:287; '
                                                    'sql/modules/OrderEntry.sql:70'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '74dab0af-b8fe-57f8-b7ba-648a54ca2155'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 537a52e774 "Fixing bug, '
                                                     'no orders show up unless closed '
                                                     'option is set" (2013-08-22); '
                                                     'commit c83b758e41 "* Fix '
                                                     'inventory adjustment entry '
                                                     'permissions problems" '
                                                     '(2017-03-03)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 2 site(s); name given by '
                      'demo-reviewer (recovered as `OeNotClosed`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    ordnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    transdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe', 'Yearend']} })
    entity_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INTEGER'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    amount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Oe']} })
    netamount: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Oe']} })
    reqdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe']} })
    taxincluded: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    shippingpoint: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    curr: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'CHAR(3)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    person_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INTEGER'}},
         'domain_of': ['Oe']} })
    closed: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Oe']} })
    quotation: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Oe']} })
    quonumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    intnotes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    shipvia: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    language_code: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(6)'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    ponumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe']} })
    terms: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT2'}},
         'domain_of': ['EntityCreditAccount', 'Oe']} })
    entity_credit_account: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Oe']} })
    oe_class_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_enum': {'tag': 'ekos_enum', 'value': 'OeOeClassId'},
                         'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Oe']} })


class Assembly(Parts):
    """
    A part built from other parts.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.4'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'parts.assembly IS TRUE'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/Goods.sql:167; '
                                                    'sql/modules/OrderEntry.sql:83'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': 'e3eb5f0a-35b4-5176-9597-046042f61fa3'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 1c69b34304 "Moved '
                                                     'order/quotation search to new '
                                                     'reporting framework." '
                                                     '(2012-12-19); commit 719a4b6d17 '
                                                     '"Allow searching parts by '
                                                     'part-type (parts, services, '
                                                     '...)" (2019-12-27)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 2 site(s); name given by '
                      'demo-reviewer (recovered as `PartsAssembly`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    partnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    unit: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(5)'}},
         'domain_of': ['Parts']} })
    listprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    sellprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    lastcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    priceupdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Parts']} })
    weight: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    onhand: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    makemodel: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    assembly: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    alternate: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    rop: Optional[Decimal] = Field(default=None, description="""Re-order point.  Used to select parts for short inventory report.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    inventory_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    income_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    expense_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    returns_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    bin: Optional[str] = Field(default=None, description="""Text identifier for where a part is stored.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    obsolete: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account', 'Parts']} })
    bom: Optional[bool] = Field(default=None, description="""Show on Bill of Materials.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    image: Optional[str] = Field(default=None, description="""Hyperlink to product image.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    drawing: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    microfiche: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    partsgroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    avgcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })


class ActivePart(Parts):
    """
    A part still offered: not obsolete.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.5'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'parts.obsolete IS FALSE'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/Goods.sql:177; '
                                                    'sql/modules/Goods.sql:299; '
                                                    'sql/modules/Parts.sql:22; '
                                                    'sql/modules/Timecards.sql:86'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '3aec3aa7-3fc0-51fd-b90f-a91baa49d3c1'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 22dc5a6c52 "Consolidated '
                                                     'goods and services search" '
                                                     '(2012-12-29); commit 6604d46dc6 '
                                                     '"More timecards work.  Working '
                                                     'on ensuring that time cards can '
                                                     'also be material cards, or labor '
                                                     'cards, for future addons." '
                                                     '(2012-12-08); commit 7794385505 '
                                                     '"A few bugs fixed with dynatable '
                                                     'and the parts search routine" '
                                                     '(2012-03-23); commit e790d8b120 '
                                                     '"Correcting load errors for '
                                                     'sql/modules/Goods.sql for '
                                                     'inventory reports" (2013-01-10)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 4 site(s); name given by '
                      'demo-reviewer (recovered as `PartsNotObsolete`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    partnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    unit: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(5)'}},
         'domain_of': ['Parts']} })
    listprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    sellprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    lastcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    priceupdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Parts']} })
    weight: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    onhand: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    makemodel: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    assembly: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    alternate: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    rop: Optional[Decimal] = Field(default=None, description="""Re-order point.  Used to select parts for short inventory report.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    inventory_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    income_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    expense_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    returns_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    bin: Optional[str] = Field(default=None, description="""Text identifier for where a part is stored.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    obsolete: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account', 'Parts']} })
    bom: Optional[bool] = Field(default=None, description="""Show on Bill of Materials.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    image: Optional[str] = Field(default=None, description="""Hyperlink to product image.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    drawing: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    microfiche: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    partsgroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    avgcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })


class InventoryPart(Parts):
    """
    A part stocked as goods: it has an inventory account.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.6'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'parts.inventory_accno_id IS NOT '
                                                      'NULL'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'sql/modules/COGS.sql:334; '
                                                    'sql/modules/COGS.sql:348; '
                                                    'sql/modules/COGS.sql:370; '
                                                    'sql/modules/COGS.sql:383; '
                                                    'sql/modules/COGS.sql:480; '
                                                    'sql/modules/Drafts.sql:132'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '0066f7b2-e21b-5137-b30c-367b6da0c661'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 08c8419a34 "When deleting '
                                                     'invoice lines, put the quatity '
                                                     'back into ONHAND" (2025-07-30); '
                                                     'commit 19c61dc19f "COGS tests '
                                                     'now passing for cases where AR '
                                                     'invoices are added first" '
                                                     '(2012-05-31); commit b6fc389859 '
                                                     '"Skip COGS calculation when '
                                                     'posting a service" (2019-11-02)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 9 site(s); name given by '
                      'demo-reviewer (recovered as `PartsWithInventoryAccnoId`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'serial'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    partnumber: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    description: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Parts']} })
    unit: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'VARCHAR(5)'}},
         'domain_of': ['Parts']} })
    listprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    sellprice: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    lastcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    priceupdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Parts']} })
    weight: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    onhand: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    notes: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Oe', 'Parts']} })
    makemodel: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    assembly: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    alternate: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    rop: Optional[Decimal] = Field(default=None, description="""Re-order point.  Used to select parts for short inventory report.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })
    inventory_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    income_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    expense_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    returns_accno_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    bin: Optional[str] = Field(default=None, description="""Text identifier for where a part is stored.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    obsolete: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Account', 'Parts']} })
    bom: Optional[bool] = Field(default=None, description="""Show on Bill of Materials.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Parts']} })
    image: Optional[str] = Field(default=None, description="""Hyperlink to product image.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    drawing: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    microfiche: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Parts']} })
    partsgroup_id: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Parts']} })
    avgcost: Optional[Decimal] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'NUMERIC'}},
         'domain_of': ['Parts']} })


class ApprovedTransaction(Transactions):
    """
    A posted transaction: approved, so it counts in reports.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.6'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'transactions.approved IS TRUE'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'lib/LedgerSMB/Database/PostUpgrade/CogsAllocation.pm:43; '
                                                    'sql/changes/1.11/cogs-allocation.sql:24; '
                                                    'sql/changes/1.11/cogs-allocation.sql:62; '
                                                    'sql/modules/Account.sql:53; '
                                                    'sql/modules/Account.sql:730; '
                                                    'sql/modules/COGS.sql:78'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '321f998e-36da-541d-9aff-86a1a27e8fea'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 15aaf32430 "Split GL and '
                                                     'non-GL transactions" '
                                                     '(2026-02-07); commit 5d8de3a41f '
                                                     '"Convert COGS routines from '
                                                     'ar/ap+gl to transactions" '
                                                     '(2026-02-07); commit fe0913bed8 '
                                                     '"COGS repairs after fixing '
                                                     'double allocations" '
                                                     '(2025-07-31); commit ffac6512f9 '
                                                     '"Correcting some load errors on '
                                                     'sql modules, merging chart.sql '
                                                     'into Account.sql" (2012-11-25)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 28 site(s); name given by '
                      'demo-reviewer (recovered as `TransactionsApproved`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Account', 'EntityCreditAccount', 'Oe', 'Parts', 'Transactions']} })
    table_name: Optional[str] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'TEXT'}},
         'domain_of': ['Transactions']} })
    locked_by: Optional[int] = Field(default=None, description="""This should only be used in pessimistic locking measures as required by large
batch work flows.""", json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Transactions']} })
    approved: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Transactions']} })
    approved_by: Optional[int] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Transactions']} })
    approved_at: Optional[datetime ] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type',
                                           'value': 'TIMESTAMP'}},
         'domain_of': ['Transactions']} })


class ActiveYearEnd(Yearend):
    """
    Rows of `yearend` where yearend.reversed IS FALSE.
    """
    linkml_meta: ClassVar[LinkMLMeta] = LinkMLMeta({'annotations': {'ekos_confidence': {'tag': 'ekos_confidence', 'value': '0.5'},
                         'ekos_definition': {'tag': 'ekos_definition',
                                             'value': 'yearend.reversed IS FALSE'},
                         'ekos_evidence': {'tag': 'ekos_evidence',
                                           'value': 'lib/LedgerSMB/Scripts/account.pm:279; '
                                                    'sql/modules/FinStatements.sql:277; '
                                                    'sql/modules/FinStatements.sql:287; '
                                                    'sql/modules/FinStatements.sql:401; '
                                                    'sql/modules/FinStatements.sql:408; '
                                                    'sql/modules/FinStatements.sql:636'},
                         'ekos_id': {'tag': 'ekos_id',
                                     'value': '49327464-4fb0-50da-b0b5-3b32de54dfaa'},
                         'ekos_origin': {'tag': 'ekos_origin', 'value': 'recurring'},
                         'ekos_rationale': {'tag': 'ekos_rationale',
                                            'value': 'commit 2c0672ae19 "Fix '
                                                     'regression preventing '
                                                     'cross-yearend reporting" '
                                                     '(2022-10-27); commit 2e80e1e325 '
                                                     '"Revisit the period and year-end '
                                                     'closing UI" (2023-07-24); commit '
                                                     '85b9685fde "Follow-up to fix '
                                                     '#835: Take year-end transactions '
                                                     'into account" (2021-01-24); '
                                                     'commit d4193ba29f "In the Income '
                                                     'Statement, use the closing '
                                                     'balance of the end date" '
                                                     '(2021-09-05)'},
                         'ekos_status': {'tag': 'ekos_status', 'value': 'confirmed'}},
         'comments': ['EKOS confirmed recovered from 7 site(s); name given by '
                      'demo-reviewer (recovered as `YearendNotReversed`).'],
         'from_schema': 'https://w3id.org/ekos/ledgersmb'})

    trans_id: int = Field(default=..., json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'INT'}},
         'domain_of': ['Yearend']} })
    reversed: Optional[bool] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'BOOL'}},
         'domain_of': ['Yearend']} })
    transdate: Optional[date] = Field(default=None, json_schema_extra = { "linkml_meta": {'annotations': {'ekos_sql_type': {'tag': 'ekos_sql_type', 'value': 'DATE'}},
         'domain_of': ['Oe', 'Yearend']} })


# Model rebuild
# see https://pydantic-docs.helpmanual.io/usage/models/#rebuilding-a-model
Account.model_rebuild()
EntityCreditAccount.model_rebuild()
Oe.model_rebuild()
Parts.model_rebuild()
Transactions.model_rebuild()
Yearend.model_rebuild()
CustomerAccount.model_rebuild()
VendorAccount.model_rebuild()
OpenOrder.model_rebuild()
Assembly.model_rebuild()
ActivePart.model_rebuild()
InventoryPart.model_rebuild()
ApprovedTransaction.model_rebuild()
ActiveYearEnd.model_rebuild()
