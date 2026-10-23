-- RFC 0211 Part 0 census: the denominator every Oracle coverage number is a fraction of.
-- Run as SYSTEM (not as ekos_ro: the census must not depend on the connector's own privileges):
--   docker exec -i ekos-oracle-xe-1 sqlplus -S -L system/ekos-local-only@localhost:1521/XEPDB1 \
--     < evals/oracle-idempiere/part0/census.sql
SET PAGESIZE 500 LINESIZE 200 FEEDBACK OFF
COLUMN object_type FORMAT A22
COLUMN what FORMAT A28
SELECT banner_full FROM v$version;
SELECT value AS characterset FROM nls_database_parameters WHERE parameter = 'NLS_CHARACTERSET';
SELECT object_type,
       SUM(CASE WHEN status = 'VALID' THEN 1 ELSE 0 END) AS valid,
       SUM(CASE WHEN status <> 'VALID' THEN 1 ELSE 0 END) AS invalid
  FROM dba_objects
 WHERE owner = 'ADEMPIERE' AND object_name NOT LIKE 'BIN$%' AND subobject_name IS NULL
 GROUP BY object_type ORDER BY 1;
SELECT 'table columns' AS what, COUNT(*) AS n
  FROM dba_tab_columns c JOIN dba_tables t ON t.owner = c.owner AND t.table_name = c.table_name
 WHERE c.owner = 'ADEMPIERE'
UNION ALL
SELECT 'view columns', COUNT(*)
  FROM dba_tab_columns c JOIN dba_views v ON v.owner = c.owner AND v.view_name = c.table_name
 WHERE c.owner = 'ADEMPIERE'
UNION ALL
SELECT 'constraints ' || constraint_type, COUNT(*) FROM dba_constraints
 WHERE owner = 'ADEMPIERE' AND table_name NOT LIKE 'BIN$%' GROUP BY constraint_type
UNION ALL
SELECT 'views longer than 4000', COUNT(*) FROM dba_views WHERE owner = 'ADEMPIERE' AND text_length > 4000
UNION ALL
SELECT 'recycle bin', COUNT(*) FROM dba_recyclebin WHERE owner = 'ADEMPIERE'
UNION ALL
SELECT 'rows (optimizer statistics)', SUM(num_rows) FROM dba_tables WHERE owner = 'ADEMPIERE';
SELECT object_type, object_name FROM dba_objects
 WHERE owner = 'ADEMPIERE' AND status <> 'VALID' ORDER BY 1, 2;
