-- RFC 0146 fixture — every construct measured on LedgerSMB's sql/Pg-database.sql that
-- sqlparser 0.53's PostgreSqlDialect cannot parse, in the shapes it actually writes them.
--
-- Hand-authored rather than excerpted from LedgerSMB (GPL) so no third-party source is vendored
-- into this repo. Eight tables; every one of them must survive `preprocess`.

\echo 'loading base schema'
\set ON_ERROR_STOP on

begin;

-- Base sections for modules and roles

CREATE TABLE lsmb_module (
     id int not null unique,
     label text primary key
);

COMMENT ON TABLE lsmb_module IS
$$ This stores categories functionality into modules.  Addons may add rows here, but
the id should be hardcoded.  As always 900-1000 will be reserved for internal use,
and negative numbers will be reserved for testing.$$;

INSERT INTO lsmb_module (id, label)
VALUES (1, 'AR'),
       (2, 'AP'),
       (3, 'GL');

CREATE TABLE language (
  code varchar(6) PRIMARY KEY,
  description text
);

COMMENT ON COLUMN language.code IS $$ ISO 639 code; may include a region suffix. $$;

CREATE SEQUENCE id;

CREATE TABLE account (
  id int primary key,
  accno text not null,
  description text,
  is_temp bool not null default false
);

-- Moving this comment to SQL comments because it is about this code rather than
-- the database structure as API. --CT
-- This could probably be done better.
COMMENT ON TABLE account IS $$ Hardwired classifications for orders and quotations.
Note that the semicolons in this sentence; and this one; must not end the statement. $$;

CREATE TABLE translation (
  trans_id int not null,
  language_code varchar(6) not null,
  description text
);

CREATE TABLE account_translation (
  PRIMARY KEY (trans_id, language_code)
) INHERITS (translation);

CREATE TABLE asset_note (
  id int primary key,
  note text
);

alter table asset_note no inherit note;

CREATE TABLE payroll_wage (
  id int primary key,
  entity_id int not null,
  rate numeric
);

CREATE TABLE entity (
  id int primary key,
  name text,
  entity_class int not null
);

CREATE TABLE defaults (
  setting_key text primary key,
  value text
);

COMMENT ON TABLE defaults IS $$ Stores global configuration. $$;

COPY defaults FROM stdin WITH DELIMITER '|';
timeout|90 minutes
sinumber|1
sonumber|1
businessnumber|1
version|1.14.0-dev
audittrail|0
\.

CREATE OR REPLACE FUNCTION person__get_my_entity_id() RETURNS INT AS
$$ SELECT -1;$$ LANGUAGE SQL;

COMMENT ON FUNCTION person__get_my_entity_id() IS
$$ Returns the entity id of the current user.  Defined twice in this file; the first
definition is a stub needed by later DEFAULT expressions. $$;

CREATE OR REPLACE FUNCTION chart_list_all() RETURNS SETOF account AS
$$ SELECT * FROM account ORDER BY accno; $$ LANGUAGE SQL;

CREATE OR REPLACE FUNCTION eca_bu_trigger() RETURNS TRIGGER AS
$$
DECLARE
  t_reference text;
  t_id int;
BEGIN
  IF TG_OP = 'INSERT' THEN
    t_id := NEW.id;
    INSERT INTO business_unit(class_id, control_code) VALUES (1, t_reference);
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;

SELECT migrate_to_identity(table_name_in := 'acc_trans', column_name_in := 'entry_id');

DO $$
DECLARE f record;
BEGIN
  FOR f IN SELECT 1 AS n LOOP
    RAISE NOTICE 'dropping %', f.n;
  END LOOP;
END;
$$;

CREATE RULE file_sec_insert_tx_oe AS ON INSERT TO file_secondary_attachment
  WHERE source_class = 1 and dest_class = 2
  DO INSTEAD INSERT INTO file_tx_to_order(file_id, ref_key) VALUES (NEW.file_id, NEW.ref_key);

commit;
