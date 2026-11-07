//! SQL column types → Ossie `DataType`. Matching is by the type's head word, never by substring
//! (`interval` and `point` contain `int`). An unknown type gives `None`, so `datatype` is omitted
//! (the spec: "omit `datatype` when the type is unknown"); a known type outside the portable
//! vocabulary — JSON, binary, UUID, arrays, intervals — is `Opaque`.

use crate::model::DataType;

/// ClickHouse wrappers that do not change the value type.
fn unwrap(t: &str) -> &str {
    for w in ["nullable(", "lowcardinality(", "simpleaggregatefunction("] {
        if let Some(inner) = t.strip_prefix(w).and_then(|r| r.strip_suffix(')')) {
            return unwrap(inner.trim());
        }
    }
    t
}

/// The data type of a SQL type name as the ledger records it (`data_type`).
pub fn from_sql(sql_type: &str) -> Option<DataType> {
    let lower = sql_type.trim().to_ascii_lowercase();
    let t = unwrap(&lower);
    if t.is_empty() {
        return None;
    }
    if t.ends_with("[]") || t.starts_with("array") || t.starts_with("map(") {
        return Some(DataType::Opaque);
    }
    let (head, args) = match t.split_once('(') {
        Some((h, rest)) => (h.trim(), rest.trim_end_matches(')').trim()),
        None => (t, ""),
    };
    let with_tz = t.contains("with time zone") || t.contains("with local time zone");
    let word = head.split_whitespace().next().unwrap_or(head);
    use DataType::*;
    let dt = match word {
        "bool" | "boolean" | "bit" if word != "bit" || args.is_empty() || args == "1" => Boolean,
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "int2" | "int4"
        | "int8" | "int16" | "int32" | "int64" | "int128" | "int256" | "uint8" | "uint16"
        | "uint32" | "uint64" | "uint128" | "uint256" | "serial" | "smallserial" | "bigserial"
        | "serial2" | "serial4" | "serial8" | "pls_integer" | "binary_integer" => Integer,
        // Oracle `NUMBER(p)` / `NUMBER(p, 0)` holds integers; `NUMBER` alone or with a scale does not.
        "number" => match args.split(',').map(str::trim).collect::<Vec<_>>()[..] {
            [p] if !p.is_empty() && p != "*" => Integer,
            [_, "0"] => Integer,
            _ => Decimal,
        },
        "numeric" | "decimal" | "dec" | "money" | "smallmoney" | "decimal32" | "decimal64"
        | "decimal128" | "decimal256" => Decimal,
        "real" | "float" | "float4" | "float8" | "float32" | "float64" | "double"
        | "binary_float" | "binary_double" => Float,
        "date" | "date32" => Date,
        "time" | "timetz" if word == "time" && !with_tz => Time,
        "timestamptz" | "datetimeoffset" => DateTimeTz,
        "timestamp" | "datetime" | "datetime2" | "smalldatetime" | "datetime64" => {
            // ClickHouse `DateTime('UTC')` / `DateTime64(3, 'UTC')` name a timezone.
            if with_tz || args.contains('\'') {
                DateTimeTz
            } else {
                DateTime
            }
        }
        "char" | "character" | "varchar" | "varchar2" | "nvarchar" | "nvarchar2" | "nchar"
        | "text" | "tinytext" | "mediumtext" | "longtext" | "string" | "fixedstring" | "clob"
        | "nclob" | "citext" | "name" | "bpchar" => String,
        "json" | "jsonb" | "xml" | "xmltype" | "uuid" | "uniqueidentifier" | "bytea" | "blob"
        | "binary" | "varbinary" | "raw" | "image" | "interval" | "inet" | "cidr" | "macaddr"
        | "tsvector" | "tsquery" | "point" | "geometry" | "geography" | "enum8" | "enum16"
        | "ipv4" | "ipv6" | "rowid" | "urowid" | "bfile" | "hstore" | "timetz" | "time" | "bit"
        | "tuple" | "nested" | "object" | "variant" | "dynamic" => Opaque,
        _ => return None,
    };
    Some(dt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use DataType::*;

    #[test]
    fn head_words_decide_never_substrings() {
        for (t, want) in [
            ("integer", Some(Integer)),
            ("INT", Some(Integer)),
            ("bigserial", Some(Integer)),
            ("interval", Some(Opaque)),
            ("point", Some(Opaque)),
            ("numeric(12,2)", Some(Decimal)),
            ("NUMBER(10)", Some(Integer)),
            ("NUMBER(10,0)", Some(Integer)),
            ("NUMBER(10,2)", Some(Decimal)),
            ("NUMBER", Some(Decimal)),
            ("double precision", Some(Float)),
            ("character varying(80)", Some(String)),
            ("VARCHAR2(30 BYTE)", Some(String)),
            ("text", Some(String)),
            ("date", Some(Date)),
            ("time", Some(Time)),
            ("time with time zone", Some(Opaque)),
            ("timestamp", Some(DateTime)),
            ("timestamp without time zone", Some(DateTime)),
            ("timestamp with time zone", Some(DateTimeTz)),
            ("TIMESTAMP(6) WITH LOCAL TIME ZONE", Some(DateTimeTz)),
            ("timestamptz", Some(DateTimeTz)),
            ("Nullable(DateTime64(3, 'UTC'))", Some(DateTimeTz)),
            ("LowCardinality(String)", Some(String)),
            ("UInt64", Some(Integer)),
            ("boolean", Some(Boolean)),
            ("bit(1)", Some(Boolean)),
            ("bit(8)", Some(Opaque)),
            ("jsonb", Some(Opaque)),
            ("text[]", Some(Opaque)),
            ("Array(String)", Some(Opaque)),
            ("my_domain", None),
            ("", None),
        ] {
            assert_eq!(from_sql(t), want, "{t}");
        }
    }
}
