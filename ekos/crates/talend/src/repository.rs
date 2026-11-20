//! Repository items other than jobs (RFC 0263 §"What a project is"): `talend.project`,
//! `.properties` files, context items, database connections, schemas and routines.

use roxmltree::{Document, Node};

use crate::job::{context_group, name_and_version};
use crate::model::{ContextItem, DbConnection, RepoSchema, RepoTable, Routine};
use crate::xml::{attr, attr_string, children, column, xsi_type};

/// What a `.properties` file says about its item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Properties {
    pub id: Option<String>,
    pub label: Option<String>,
    pub version: Option<String>,
    /// `ItemState/@path`: the repository folder.
    pub folder: Option<String>,
    pub purpose: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

pub(crate) fn parse_properties(text: &str) -> Result<Properties, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let mut props = Properties::default();
    for node in doc.root_element().children().filter(Node::is_element) {
        match node.tag_name().name() {
            "Property" => property(&node, &mut props),
            "ItemState" => props.folder = attr_string(&node, "path"),
            _ => {}
        }
    }
    Ok(props)
}

fn property(node: &Node, props: &mut Properties) {
    props.id = attr_string(node, "id");
    props.label = attr_string(node, "label");
    props.version = attr_string(node, "version");
    props.purpose = attr_string(node, "purpose");
    props.description = attr_string(node, "description");
    for extra in children(node, "additionalProperties") {
        let value = attr_string(&extra, "value");
        match extra.attribute("key") {
            Some("created_date") => props.created = value,
            Some("modified_date") => props.modified = value,
            _ => {}
        }
    }
}

/// `talend.project` → (label, technical label, product version).
pub(crate) fn parse_project_file(text: &str) -> Result<(String, String, Option<String>), String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let project = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Project")
        .ok_or("no Project element")?;
    Ok((
        attr_string(&project, "label").unwrap_or_default(),
        attr_string(&project, "technicalLabel").unwrap_or_default(),
        attr_string(&project, "productVersion"),
    ))
}

pub(crate) fn parse_context_item(path: &str, text: &str) -> Result<ContextItem, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let (name, version) = name_and_version(path);
    // One `ContextType` per group: as the root, or as children of an `xmi:XMI` wrapper.
    let groups = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "ContextType")
        .map(|n| context_group(&n))
        .collect();
    Ok(ContextItem {
        id: None,
        name,
        version,
        path: path.to_string(),
        groups,
    })
}

pub(crate) fn parse_connection(path: &str, text: &str) -> Result<DbConnection, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let (name, version) = name_and_version(path);
    let conn = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name().ends_with("Connection"))
        .ok_or("no connection element")?;
    Ok(DbConnection {
        id: attr_string(&conn, "id"),
        name: attr_string(&conn, "label")
            .or_else(|| attr_string(&conn, "name"))
            .unwrap_or(name),
        version,
        path: path.to_string(),
        engine: attr_string(&conn, "DatabaseType"),
        host: attr_string(&conn, "ServerName"),
        port: attr_string(&conn, "Port"),
        database: attr_string(&conn, "SID"),
        schema: attr_string(&conn, "UiSchema"),
        username: attr_string(&conn, "Username"),
        context_id: (conn.attribute("ContextMode") == Some("true"))
            .then(|| attr_string(&conn, "ContextId"))
            .flatten(),
        has_password: attr(&conn, "Password").is_some(),
        tables: tables(&doc),
    })
}

pub(crate) fn parse_schema(path: &str, family: &str, text: &str) -> Result<RepoSchema, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let (name, version) = name_and_version(path);
    Ok(RepoSchema {
        id: None,
        name,
        version,
        path: path.to_string(),
        family: family.to_string(),
        tables: tables(&doc),
    })
}

/// Every table-like element (`relational:TdTable`, `TalendMetadata:MetadataTable`) with its columns.
fn tables(doc: &Document) -> Vec<RepoTable> {
    doc.descendants()
        .filter(|n| {
            n.is_element()
                && xsi_type(n)
                    .is_some_and(|t| t.ends_with(":TdTable") || t.ends_with(":MetadataTable"))
        })
        .map(|t| RepoTable {
            name: attr_string(&t, "label")
                .or_else(|| attr_string(&t, "name"))
                .unwrap_or_default(),
            columns: children(&t, "feature").map(|c| column(&c)).collect(),
        })
        .collect()
}

/// A routine item is the routine's Java source as is.
pub(crate) fn parse_routine(path: &str, text: &str, system: bool) -> Routine {
    let (name, version) = name_and_version(path);
    Routine {
        name,
        version,
        path: path.to_string(),
        system,
        methods: if system {
            Vec::new()
        } else {
            static_methods(text)
        },
        source: (!system).then(|| text.to_string()),
    }
}

/// Names of `public static` methods, in order of declaration.
fn static_methods(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("public static ") else {
            continue;
        };
        let Some(open) = rest.find('(') else {
            continue;
        };
        let name = rest[..open].split_whitespace().last().unwrap_or_default();
        if !name.is_empty() && !out.iter().any(|m| m == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_give_the_id_folder_and_dates() {
        let p = parse_properties(
            r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties">
  <TalendProperties:Property id="_job1" label="jLoad" version="0.2" purpose="loads">
    <additionalProperties key="created_date" value="2025-06-03"/>
  </TalendProperties:Property>
  <TalendProperties:ItemState path="ODS/sub"/>
</xmi:XMI>"#,
        )
        .unwrap();
        assert_eq!(p.id.as_deref(), Some("_job1"));
        assert_eq!(p.folder.as_deref(), Some("ODS/sub"));
        assert_eq!(p.purpose.as_deref(), Some("loads"));
        assert_eq!(p.created.as_deref(), Some("2025-06-03"));
    }

    #[test]
    fn a_connection_keeps_its_settings_and_tables_but_no_password() {
        let c = parse_connection(
            "metadata/connections/ODS_0.1.item",
            r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMetadata="http://www.talend.org/metadata/connection/2010" xmlns:relational="http://www.talend.org/cwm/resource/relational/2010">
  <TalendMetadata:DatabaseConnection id="_c1" label="ODS" DatabaseType="PostgreSQL" ServerName="context.ServerName" Port="context.port" SID="context.database" UiSchema="context.schema_ods" Username="context.user" Password="secret-text" ContextMode="true" ContextId="_ctx1">
    <dataPackage>
      <ownedElement xsi:type="relational:TdTable" name="ODS_T" label="ODS_T">
        <feature xsi:type="relational:TdColumn" name="ID" talendType="id_String" sourceType="VARCHAR" key="true" length="50"/>
      </ownedElement>
    </dataPackage>
  </TalendMetadata:DatabaseConnection>
</xmi:XMI>"#,
        )
        .unwrap();
        assert_eq!(c.engine.as_deref(), Some("PostgreSQL"));
        assert_eq!(c.schema.as_deref(), Some("context.schema_ods"));
        assert_eq!(c.context_id.as_deref(), Some("_ctx1"));
        assert!(c.has_password);
        assert_eq!(c.tables[0].name, "ODS_T");
        assert!(c.tables[0].columns[0].key);
        assert!(!format!("{c:?}").contains("secret-text"));
    }

    #[test]
    fn a_custom_routine_lists_its_static_methods() {
        let r = parse_routine(
            "code/routines/customCode_0.1.item",
            "package routines;\npublic class customCode {\n    public static String clean(String s) { return s; }\n    public static int n() { return 1; }\n}\n",
            false,
        );
        assert_eq!(r.methods, vec!["clean", "n"]);
        assert!(
            parse_routine("code/routines/system/Numeric_0.1.item", "x", true)
                .source
                .is_none()
        );
    }
}
