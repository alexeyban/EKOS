//! Small `roxmltree` helpers shared by the readers.

use roxmltree::Node;

pub(crate) const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// An attribute's value, `None` when absent or empty.
pub(crate) fn attr<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute(name).filter(|v| !v.is_empty())
}

pub(crate) fn attr_string(node: &Node, name: &str) -> Option<String> {
    attr(node, name).map(str::to_string)
}

pub(crate) fn attr_bool(node: &Node, name: &str) -> bool {
    node.attribute(name) == Some("true")
}

pub(crate) fn attr_i64(node: &Node, name: &str) -> Option<i64> {
    attr(node, name)?.parse().ok()
}

/// `xsi:type`, e.g. `TalendMapper:MapperData`.
pub(crate) fn xsi_type<'a>(node: &Node<'a, '_>) -> Option<&'a str> {
    node.attribute((XSI, "type"))
}

/// Element children with the given local name.
pub(crate) fn children<'a, 'i>(
    node: &Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// 1-based line of the element's start tag.
pub(crate) fn line(doc: &roxmltree::Document, node: &Node) -> u32 {
    doc.text_pos_at(node.range().start).row
}

/// `talendType="id_String"` style column attributes into a [`crate::Column`].
pub(crate) fn column(node: &Node) -> crate::Column {
    crate::Column {
        name: attr_string(node, "name")
            .or_else(|| attr_string(node, "label"))
            .unwrap_or_default(),
        talend_type: attr_string(node, "type").or_else(|| attr_string(node, "talendType")),
        db_type: attr_string(node, "sourceType"),
        key: attr_bool(node, "key"),
        // Talend omits `nullable` when it is false on repository columns and writes it on
        // job columns; absent means "not stated", read as nullable only when written.
        nullable: attr_bool(node, "nullable"),
        length: attr_i64(node, "length").filter(|n| *n >= 0),
        precision: attr_i64(node, "precision").filter(|n| *n >= 0),
        pattern: attr_string(node, "pattern"),
        comment: attr_string(node, "comment"),
    }
}
