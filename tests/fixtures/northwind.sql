-- Northwind fixture schema (Phase 13/14 test-data pass — near-real, open-source data).
--
-- Adapted from Microsoft's Northwind sample database:
--   https://github.com/microsoft/sql-server-samples/blob/master/samples/databases/northwind-pubs/instnwnd.sql
-- Original work: Copyright (c) Microsoft Corporation. Licensed under the MIT License
-- (see https://github.com/microsoft/sql-server-samples/blob/master/license.txt).
--
-- This file is a hand-cleaned ANSI-SQL subset of the original T-SQL script: table and
-- column names, types, and primary/foreign-key relationships are preserved from the
-- real Northwind schema; SQL-Server-only syntax (GO batch separators, bracket-quoted
-- identifiers, IDENTITY columns, CHECK/DEFAULT constraints, indexes, and views) has
-- been stripped or converted to portable ANSI SQL so `sqlparser`'s GenericDialect can
-- parse it. Five relationships not declared as formal FOREIGN KEY constraints in the
-- original script (EmployeeTerritories → Employees/Territories, Territories → Region,
-- CustomerCustomerDemo → Customers/CustomerDemographics) are the schema's real,
-- documented relational structure — made explicit here as FK constraints since this
-- fixture exists to exercise identity resolution / semantic compilation over a
-- realistically deep FK graph (13 tables), not just to reproduce the original script
-- byte-for-byte.
--
-- Not included: sample data rows (the original script also seeds ~3000 rows of
-- Northwind's classic sample data — irrelevant to structural schema recovery, so this
-- fixture is schema-only, mirroring how `ecommerce.sql` is also schema-only).

CREATE TABLE Employees (
	EmployeeID int NOT NULL,
	LastName varchar(20) NOT NULL,
	FirstName varchar(10) NOT NULL,
	Title varchar(30),
	TitleOfCourtesy varchar(25),
	BirthDate date,
	HireDate date,
	Address varchar(60),
	City varchar(15),
	Region varchar(15),
	PostalCode varchar(10),
	Country varchar(15),
	HomePhone varchar(24),
	Extension varchar(4),
	Notes text,
	ReportsTo int,
	PhotoPath varchar(255),
	PRIMARY KEY (EmployeeID),
	FOREIGN KEY (ReportsTo) REFERENCES Employees (EmployeeID)
);

CREATE TABLE Categories (
	CategoryID int NOT NULL,
	CategoryName varchar(15) NOT NULL,
	Description text,
	PRIMARY KEY (CategoryID)
);

CREATE TABLE Customers (
	CustomerID char(5) NOT NULL,
	CompanyName varchar(40) NOT NULL,
	ContactName varchar(30),
	ContactTitle varchar(30),
	Address varchar(60),
	City varchar(15),
	Region varchar(15),
	PostalCode varchar(10),
	Country varchar(15),
	Phone varchar(24),
	Fax varchar(24),
	PRIMARY KEY (CustomerID)
);

CREATE TABLE Shippers (
	ShipperID int NOT NULL,
	CompanyName varchar(40) NOT NULL,
	Phone varchar(24),
	PRIMARY KEY (ShipperID)
);

CREATE TABLE Suppliers (
	SupplierID int NOT NULL,
	CompanyName varchar(40) NOT NULL,
	ContactName varchar(30),
	ContactTitle varchar(30),
	Address varchar(60),
	City varchar(15),
	Region varchar(15),
	PostalCode varchar(10),
	Country varchar(15),
	Phone varchar(24),
	Fax varchar(24),
	HomePage text,
	PRIMARY KEY (SupplierID)
);

CREATE TABLE Orders (
	OrderID int NOT NULL,
	CustomerID char(5),
	EmployeeID int,
	OrderDate date,
	RequiredDate date,
	ShippedDate date,
	ShipVia int,
	Freight decimal(10, 2),
	ShipName varchar(40),
	ShipAddress varchar(60),
	ShipCity varchar(15),
	ShipRegion varchar(15),
	ShipPostalCode varchar(10),
	ShipCountry varchar(15),
	PRIMARY KEY (OrderID),
	FOREIGN KEY (CustomerID) REFERENCES Customers (CustomerID),
	FOREIGN KEY (EmployeeID) REFERENCES Employees (EmployeeID),
	FOREIGN KEY (ShipVia) REFERENCES Shippers (ShipperID)
);

CREATE TABLE Products (
	ProductID int NOT NULL,
	ProductName varchar(40) NOT NULL,
	SupplierID int,
	CategoryID int,
	QuantityPerUnit varchar(20),
	UnitPrice decimal(10, 2),
	UnitsInStock smallint,
	UnitsOnOrder smallint,
	ReorderLevel smallint,
	Discontinued boolean NOT NULL,
	PRIMARY KEY (ProductID),
	FOREIGN KEY (CategoryID) REFERENCES Categories (CategoryID),
	FOREIGN KEY (SupplierID) REFERENCES Suppliers (SupplierID)
);

CREATE TABLE "Order Details" (
	OrderID int NOT NULL,
	ProductID int NOT NULL,
	UnitPrice decimal(10, 2) NOT NULL,
	Quantity smallint NOT NULL,
	Discount real NOT NULL,
	PRIMARY KEY (OrderID, ProductID),
	FOREIGN KEY (OrderID) REFERENCES Orders (OrderID),
	FOREIGN KEY (ProductID) REFERENCES Products (ProductID)
);

CREATE TABLE Region (
	RegionID int NOT NULL,
	RegionDescription char(50) NOT NULL,
	PRIMARY KEY (RegionID)
);

CREATE TABLE Territories (
	TerritoryID varchar(20) NOT NULL,
	TerritoryDescription char(50) NOT NULL,
	RegionID int NOT NULL,
	PRIMARY KEY (TerritoryID),
	FOREIGN KEY (RegionID) REFERENCES Region (RegionID)
);

CREATE TABLE EmployeeTerritories (
	EmployeeID int NOT NULL,
	TerritoryID varchar(20) NOT NULL,
	PRIMARY KEY (EmployeeID, TerritoryID),
	FOREIGN KEY (EmployeeID) REFERENCES Employees (EmployeeID),
	FOREIGN KEY (TerritoryID) REFERENCES Territories (TerritoryID)
);

CREATE TABLE CustomerDemographics (
	CustomerTypeID char(10) NOT NULL,
	CustomerDesc text,
	PRIMARY KEY (CustomerTypeID)
);

CREATE TABLE CustomerCustomerDemo (
	CustomerID char(5) NOT NULL,
	CustomerTypeID char(10) NOT NULL,
	PRIMARY KEY (CustomerID, CustomerTypeID),
	FOREIGN KEY (CustomerID) REFERENCES Customers (CustomerID),
	FOREIGN KEY (CustomerTypeID) REFERENCES CustomerDemographics (CustomerTypeID)
);
