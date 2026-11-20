//! Reading one job item (`talendfile:ProcessType`, RFC 0263 §"Job items").

use std::collections::BTreeMap;

use roxmltree::{Document, Node};

use crate::catalogue::classify;
use crate::model::{
    Component, ConnectorSchema, ContextGroup, ContextVar, Job, Link, LinkKind, Mapper, MapperEntry,
    MapperInput, MapperOutput, Param, Trigger, TriggerKind,
};
use crate::xml::{attr, attr_bool, attr_string, children, column, line, xsi_type};

/// Studio's line style for a lookup row link (`EConnectionType.FLOW_REF`).
const LOOKUP_LINE_STYLE: &str = "8";

/// Reads a job item. `path` is relative to the project root; name, version and folder come from
/// it, and the caller overlays what the `.properties` file says.
pub(crate) fn parse_job(path: &str, text: &str) -> Result<Job, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if root.tag_name().name() != "ProcessType" {
        return Err(format!(
            "not a job item: root element is <{}>",
            root.tag_name().name()
        ));
    }
    let (name, version) = name_and_version(path);
    let mut job = Job {
        name,
        version,
        folder: folder_of(path),
        path: path.to_string(),
        job_type: attr_string(&root, "jobType"),
        default_context: attr_string(&root, "defaultContext").unwrap_or_default(),
        ..Job::default()
    };
    job.context_groups = children(&root, "context")
        .map(|c| context_group(&c))
        .collect();
    job.implicit_context_load = children(&root, "parameters")
        .flat_map(|p| children(&p, "elementParameter").collect::<Vec<_>>())
        .any(|p| p.attribute("name") == Some("IMPLICIT_TCONTEXTLOAD") && attr_bool(&p, "value"));
    job.components = children(&root, "node")
        .map(|n| component(&doc, &n))
        .collect();
    for c in children(&root, "connection") {
        connection(&doc, &c, &mut job);
    }
    job.subjob_count = children(&root, "subjob").count();
    Ok(job)
}

/// `process/ODS/jOdsVente_0.1.item` → (`jOdsVente`, `0.1`).
pub(crate) fn name_and_version(path: &str) -> (String, String) {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    match stem.rsplit_once('_') {
        Some((name, version)) if version.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            (name.to_string(), version.to_string())
        }
        _ => (stem.to_string(), String::new()),
    }
}

/// `process/DWH/facts/j_0.1.item` → `DWH/facts`.
fn folder_of(path: &str) -> String {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    if !parts.is_empty() {
        parts.remove(0);
    }
    parts.join("/")
}

pub(crate) fn context_group(node: &Node) -> ContextGroup {
    ContextGroup {
        name: attr_string(node, "name").unwrap_or_default(),
        variables: children(node, "contextParameter")
            .map(|p| context_var(&p))
            .collect(),
    }
}

fn context_var(node: &Node) -> ContextVar {
    let talend_type = attr_string(node, "type").unwrap_or_default();
    let secret = talend_type == "id_Password";
    ContextVar {
        name: attr_string(node, "name").unwrap_or_default(),
        value: if secret {
            None
        } else {
            node.attribute("value").map(str::to_string)
        },
        secret,
        talend_type,
        prompt: attr_string(node, "prompt"),
        comment: attr_string(node, "comment"),
        repository_context_id: attr_string(node, "repositoryContextId"),
    }
}

fn component(doc: &Document, node: &Node) -> Component {
    let params: BTreeMap<String, Param> = children(node, "elementParameter")
        .filter_map(|p| Some((attr_string(&p, "name")?, param(&p))))
        .collect();
    let text = |name: &str| params.get(name).and_then(Param::text).map(str::to_string);
    let component_type = attr_string(node, "componentName").unwrap_or_default();
    let classified = classify(&component_type, text("TYPE").as_deref());
    Component {
        unique_name: text("UNIQUE_NAME").unwrap_or_default(),
        type_version: attr_string(node, "componentVersion"),
        category: classified.category,
        logical: classified.logical,
        engine: classified.engine,
        label: text("LABEL").filter(|l| !l.is_empty()),
        schemas: children(node, "metadata").map(|m| schema(&m)).collect(),
        mapper: children(node, "nodeData")
            .find(|d| xsi_type(d).is_some_and(|t| t.ends_with("MapperData")))
            .map(|d| mapper(&d)),
        line: line(doc, node),
        component_type,
        params,
    }
}

fn param(node: &Node) -> Param {
    let field = attr_string(node, "field").unwrap_or_default();
    if field == "PASSWORD" {
        return Param::Secret {
            set: attr(node, "value").is_some(),
            field,
        };
    }
    if field == "TABLE" {
        return Param::Table {
            rows: table_rows(node),
            field,
        };
    }
    Param::Value {
        value: node.attribute("value").unwrap_or_default().to_string(),
        field,
    }
}

/// A `TABLE` parameter's flat `elementValue` list → rows. A row ends when a column already in it
/// comes again (RFC 0263 §"Job items").
fn table_rows(node: &Node) -> Vec<BTreeMap<String, String>> {
    let mut rows = Vec::new();
    let mut row: BTreeMap<String, String> = BTreeMap::new();
    for value in children(node, "elementValue") {
        let key = attr_string(&value, "elementRef").unwrap_or_default();
        if row.contains_key(&key) {
            rows.push(std::mem::take(&mut row));
        }
        row.insert(
            key,
            value.attribute("value").unwrap_or_default().to_string(),
        );
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

fn schema(node: &Node) -> ConnectorSchema {
    ConnectorSchema {
        connector: attr_string(node, "connector").unwrap_or_default(),
        name: attr_string(node, "name").unwrap_or_default(),
        label: attr_string(node, "label"),
        columns: children(node, "column").map(|c| column(&c)).collect(),
    }
}

fn mapper(node: &Node) -> Mapper {
    Mapper {
        inputs: children(node, "inputTables")
            .map(|t| MapperInput {
                name: attr_string(&t, "name").unwrap_or_default(),
                matching_mode: attr_string(&t, "matchingMode"),
                lookup_mode: attr_string(&t, "lookupMode"),
                inner_join: attr_bool(&t, "innerJoin"),
                filter: table_filter(&t),
                entries: entries(&t),
            })
            .collect(),
        vars: children(node, "varTables")
            .flat_map(|t| entries(&t))
            .collect(),
        outputs: children(node, "outputTables")
            .map(|t| MapperOutput {
                name: attr_string(&t, "name").unwrap_or_default(),
                reject: attr_bool(&t, "reject"),
                reject_inner_join: attr_bool(&t, "rejectInnerJoin"),
                filter: table_filter(&t),
                entries: entries(&t),
            })
            .collect(),
    }
}

/// A mapper table's expression filter, when it is switched on and not empty.
fn table_filter(node: &Node) -> Option<String> {
    if !attr_bool(node, "activateExpressionFilter") {
        return None;
    }
    attr(node, "expressionFilter")
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string)
}

fn entries(node: &Node) -> Vec<MapperEntry> {
    children(node, "mapperTableEntries")
        .map(|e| MapperEntry {
            name: attr_string(&e, "name").unwrap_or_default(),
            talend_type: attr_string(&e, "type"),
            expression: attr(&e, "expression")
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string),
            operator: attr_string(&e, "operator"),
        })
        .collect()
}

fn connection(doc: &Document, node: &Node, job: &mut Job) {
    let connector = node.attribute("connectorName").unwrap_or_default();
    let from = attr_string(node, "source").unwrap_or_default();
    let to = attr_string(node, "target").unwrap_or_default();
    let at = line(doc, node);
    if let Some(kind) = trigger_kind(connector) {
        let condition = children(node, "elementParameter")
            .find(|p| p.attribute("name") == Some("CONDITION"))
            .and_then(|p| attr_string(&p, "value"));
        job.triggers.push(Trigger {
            kind,
            from,
            to,
            condition,
            line: at,
        });
        return;
    }
    let kind = match connector {
        "FLOW" if node.attribute("lineStyle") == Some(LOOKUP_LINE_STYLE) => LinkKind::Lookup,
        "FLOW" | "MAIN" => LinkKind::Main,
        "FILTER" => LinkKind::Filter,
        "REJECT" => LinkKind::Reject,
        "UNIQUE" => LinkKind::Unique,
        "DUPLICATE" => LinkKind::Duplicate,
        "ITERATE" => LinkKind::Iterate,
        other => LinkKind::Other(other.to_string()),
    };
    job.links.push(Link {
        kind,
        from,
        to,
        label: attr_string(node, "label"),
        metaname: attr_string(node, "metaname"),
        line: at,
    });
}

fn trigger_kind(connector: &str) -> Option<TriggerKind> {
    Some(match connector {
        "COMPONENT_OK" => TriggerKind::ComponentOk,
        "COMPONENT_ERROR" => TriggerKind::ComponentError,
        "SUBJOB_OK" => TriggerKind::SubjobOk,
        "SUBJOB_ERROR" => TriggerKind::SubjobError,
        "RUN_IF" => TriggerKind::RunIf,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic: written for this test in the shape of fixture B's items (RFC 0262 §Fixtures).
    pub(crate) const JOB: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<talendfile:ProcessType xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMapper="http://www.talend.org/mapper" xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default" jobType="Standard">
  <context name="Default">
    <contextParameter name="schema_ods" type="id_String" value="ODS" repositoryContextId="_ctx1"/>
    <contextParameter name="password" type="id_Password" value="enc:system.encryption.key.v1:QUJDREVGRw=="/>
  </context>
  <parameters>
    <elementParameter field="CHECK" name="IMPLICIT_TCONTEXTLOAD" value="true"/>
  </parameters>
  <node componentName="tPostgresqlInput" componentVersion="0.102">
    <elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBInput_1"/>
    <elementParameter field="TEXT" name="TYPE" value="POSTGRESQL"/>
    <elementParameter field="PASSWORD" name="PASS" value="enc:system.encryption.key.v1:SElKS0w="/>
    <elementParameter field="MEMO_SQL" name="QUERY" value="&quot;SELECT \&quot;ID\&quot; FROM \&quot;&quot;+context.schema_ods+&quot;\&quot;.\&quot;T\&quot;&quot;"/>
    <metadata connector="FLOW" name="tDBInput_1">
      <column key="true" length="10" name="ID" nullable="false" type="id_String" sourceType="VARCHAR"/>
    </metadata>
  </node>
  <node componentName="tMap" componentVersion="2.1">
    <elementParameter field="TEXT" name="UNIQUE_NAME" value="tMap_1"/>
    <nodeData xsi:type="TalendMapper:MapperData">
      <varTables name="Var"/>
      <outputTables name="out" activateExpressionFilter="true" expressionFilter=" row1.ID != null ">
        <mapperTableEntries name="ID" expression="row1.ID " type="id_String"/>
      </outputTables>
      <outputTables name="rej" reject="true" rejectInnerJoin="true">
        <mapperTableEntries name="ID" expression="row1.ID" type="id_String"/>
      </outputTables>
      <inputTables name="row1" matchingMode="UNIQUE_MATCH" lookupMode="LOAD_ONCE"/>
      <inputTables name="row2" matchingMode="UNIQUE_MATCH" innerJoin="true">
        <mapperTableEntries name="ID" expression="row1.ID" operator="=" type="id_String"/>
      </inputTables>
    </nodeData>
  </node>
  <node componentName="tAggregateRow" componentVersion="0.102">
    <elementParameter field="TEXT" name="UNIQUE_NAME" value="tAggregateRow_1"/>
    <elementParameter field="TABLE" name="OPERATIONS">
      <elementValue elementRef="OUTPUT_COLUMN" value="N"/>
      <elementValue elementRef="FUNCTION" value="count"/>
      <elementValue elementRef="INPUT_COLUMN" value="ID"/>
      <elementValue elementRef="OUTPUT_COLUMN" value="S"/>
      <elementValue elementRef="FUNCTION" value="sum"/>
      <elementValue elementRef="INPUT_COLUMN" value="AMT"/>
    </elementParameter>
  </node>
  <connection connectorName="FLOW" label="row1" lineStyle="0" metaname="tDBInput_1" source="tDBInput_1" target="tMap_1"/>
  <connection connectorName="FLOW" label="row2" lineStyle="8" metaname="tDBInput_1" source="tDBInput_2" target="tMap_1"/>
  <connection connectorName="RUN_IF" label="If" lineStyle="6" source="tFileList_1" target="tDBInput_1">
    <elementParameter field="MEMO_CODE" name="CONDITION" value="((Integer)globalMap.get(&quot;tFileList_1_NB_FILE&quot;)) == 1"/>
  </connection>
  <subjob/>
</talendfile:ProcessType>
"#;

    #[test]
    fn a_job_item_reads_into_components_links_and_triggers() {
        let job = parse_job("process/ODS/jLoad_0.2.item", JOB).unwrap();
        assert_eq!((job.name.as_str(), job.version.as_str()), ("jLoad", "0.2"));
        assert_eq!(job.folder, "ODS");
        assert!(job.implicit_context_load);
        assert_eq!(job.components.len(), 3);
        let input = &job.components[0];
        assert_eq!(input.unique_name, "tDBInput_1");
        assert_eq!(input.logical, "db_input");
        assert_eq!(input.engine.as_deref(), Some("postgresql"));
        assert_eq!(input.line, 10);
        assert_eq!(
            input.schemas[0].columns[0].db_type.as_deref(),
            Some("VARCHAR")
        );
        assert_eq!(job.links.len(), 2);
        assert_eq!(job.links[1].kind, LinkKind::Lookup);
        assert_eq!(job.triggers[0].kind, TriggerKind::RunIf);
        assert!(
            job.triggers[0]
                .condition
                .as_deref()
                .unwrap()
                .contains("NB_FILE")
        );
        assert_eq!(job.subjob_count, 1);
    }

    #[test]
    fn table_parameters_become_rows() {
        let job = parse_job("process/j_0.1.item", JOB).unwrap();
        let rows = job.components[2].params["OPERATIONS"].rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["FUNCTION"], "sum");
        assert_eq!(rows[1]["INPUT_COLUMN"], "AMT");
    }

    #[test]
    fn the_tmap_reads_inputs_lookups_filters_and_rejects() {
        let job = parse_job("process/j_0.1.item", JOB).unwrap();
        let m = job.components[1].mapper.as_ref().unwrap();
        assert_eq!(m.inputs.len(), 2);
        assert!(m.inputs[1].inner_join);
        assert_eq!(m.inputs[1].entries[0].operator.as_deref(), Some("="));
        assert_eq!(m.outputs[0].filter.as_deref(), Some("row1.ID != null"));
        assert_eq!(
            m.outputs[0].entries[0].expression.as_deref(),
            Some("row1.ID")
        );
        assert!(m.outputs[1].reject && m.outputs[1].reject_inner_join);
        assert!(m.vars.is_empty());
    }

    #[test]
    fn no_password_value_is_ever_kept() {
        let job = parse_job("process/j_0.1.item", JOB).unwrap();
        let var = &job.context_groups[0].variables[1];
        assert!(var.secret && var.value.is_none());
        assert_eq!(
            job.components[0].params["PASS"],
            Param::Secret {
                field: "PASSWORD".into(),
                set: true
            }
        );
        let text = format!("{job:?}");
        assert!(!text.contains("enc:system"), "{text}");
    }

    #[test]
    fn a_file_that_is_not_a_job_is_refused() {
        assert!(parse_job("process/x_0.1.item", "<a/>").is_err());
        assert!(parse_job("process/x_0.1.item", "<a").is_err());
    }
}
