//! Parsing `_panels` facts into the [`Panel`] model.
//!
//! Two input shapes are accepted:
//!
//! * **Raw solution rows** from `logic.query` against `_panels`, one goal per
//!   field of [`PanelFacts`] — see [`Panel::all_from_facts`].
//! * **`smart_panel.list` output**, the gateway's own summary of every panel
//!   with props, field ids and a data preview — see [`Panel::all_from_list`].
//!   This is the intended client path; it spares a consumer seven queries and
//!   the paging that a large account's props require.
//!
//! This module is deliberately tolerant: a panel with a malformed prop is a
//! panel with one fewer prop, not an error, because a dashboard that refuses to
//! draw is worse than one that draws incompletely.
//!
//! # Fetch-layer facts every caller must know
//!
//! These belong to whoever runs the queries, not to this crate, but the parser
//! is shaped around them:
//!
//! * **An undefined predicate is an error, not an empty set.** A cell in which
//!   no panel has ever declared a source or a field will reject
//!   `panel_source(...)` outright. Treat that error as "no rows".
//! * **Large results are not returned inline.** Past ~48 KB the gateway
//!   returns a preview plus a `cache_ref`; the rows are retrieved with
//!   `prism.paginate`, which for a `logic.query` result pages the *solution
//!   rows* (`per_page` up to 10,000). A whole account's `panel_prop` facts
//!   already exceed the inline budget.
//! * **Registrations repeat.** Republishing can leave several identical
//!   `panel/3` rows and several `parent` edges for the same panel. Identity
//!   is `(namespace, id)`; duplicates are collapsed.
//!
//! # Why rows are messy
//!
//! Values cross the Prolog term boundary in several shapes: real JSON values,
//! chart-style `[label, value]` lists, and Prolog-rendered curly-term *strings*
//! like `"{amount:420,claim:clm_501}"` — map data that lost its typing in
//! transit. [`normalize_rows`] flattens all three into the same
//! `Vec<Vec<Value>>`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::{
    prop_bool, Field, FieldEmit, FieldTrigger, Panel, PanelKind, PanelSource, Props, TriggerEvent,
    TriggerType,
};

/// The goals this crate expects a caller to run against `_panels`. Provided so
/// every consumer queries the same way rather than each inventing its own.
///
/// Any of these may fail with "predicate not defined" in a cell where nothing
/// has asserted that predicate yet. That is an empty result, not a failure.
pub mod goals {
    pub const PANELS: &str = "panel(Id, Kind, Namespace)";
    pub const PROPS: &str = "panel_prop(Id, Key, Value)";
    pub const DATA: &str = "panel_data(Id, Rows)";
    pub const SOURCE: &str = "panel_source(Id, Ns, Query)";
    pub const FIELD_INPUT: &str = "field_input(FieldId, DependsOn)";
    pub const FIELD_TRIGGER: &str = "field_trigger(FieldId, Type, Event)";
    pub const FIELD_EMIT: &str = "field_emit(FieldId, EmitType)";
}

/// Raw solution rows for each `_panels` goal, as returned by `logic.query`.
///
/// Callers fetch these however they like — MCP, a local proxy, or a fixture
/// file. Keeping the fetch outside this crate is what keeps it free of any
/// transport dependency, so one model serves a GUI, a transpiler, and a test
/// with no shared plumbing.
#[derive(Debug, Default, Clone)]
pub struct PanelFacts {
    pub panels: Vec<Value>,
    pub props: Vec<Value>,
    pub data: Vec<Value>,
    pub sources: Vec<Value>,
    pub field_inputs: Vec<Value>,
    pub field_triggers: Vec<Value>,
    pub field_emits: Vec<Value>,
}

impl Panel {
    /// Build every panel present in `facts`.
    ///
    /// Parts (form fields, dashboard children) are folded into their container
    /// and removed from the top-level result, so callers get panels, not
    /// panels-and-their-parts.
    pub fn all_from_facts(facts: &PanelFacts) -> Vec<Panel> {
        let mut props_by_id: BTreeMap<String, Props> = BTreeMap::new();
        for row in &facts.props {
            let (Some(id), Some(k)) = (str_at(row, "Id"), str_at(row, "Key")) else {
                continue;
            };
            let Some(v) = row.get("Value") else { continue };
            // Repeated prop facts for the same key: last one wins, which is
            // also what a map of them does on the server.
            props_by_id.entry(id).or_default().insert(k, v.clone());
        }

        let mut source_by_id: BTreeMap<String, PanelSource> = BTreeMap::new();
        for row in &facts.sources {
            let (Some(id), Some(ns), Some(q)) =
                (str_at(row, "Id"), str_at(row, "Ns"), str_at(row, "Query"))
            else {
                continue;
            };
            source_by_id.insert(
                id,
                PanelSource {
                    namespace: ns,
                    query: q,
                },
            );
        }

        let mut data_by_id: BTreeMap<String, Vec<Vec<Value>>> = BTreeMap::new();
        for row in &facts.data {
            let Some(id) = str_at(row, "Id") else {
                continue;
            };
            let rows = row.get("Rows").cloned().unwrap_or(Value::Null);
            data_by_id.insert(id, normalize_rows(&rows));
        }

        let mut inputs_by_field: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in &facts.field_inputs {
            let (Some(f), Some(dep)) = (str_at(row, "FieldId"), str_at(row, "DependsOn")) else {
                continue;
            };
            let deps = inputs_by_field.entry(f).or_default();
            if !deps.contains(&dep) {
                deps.push(dep);
            }
        }

        let mut trigger_by_field: BTreeMap<String, FieldTrigger> = BTreeMap::new();
        for row in &facts.field_triggers {
            let (Some(f), Some(t), Some(e)) = (
                str_at(row, "FieldId"),
                str_at(row, "Type"),
                str_at(row, "Event"),
            ) else {
                continue;
            };
            trigger_by_field.insert(
                f,
                FieldTrigger {
                    trigger_type: TriggerType::parse(&t),
                    event: TriggerEvent::parse(&e),
                },
            );
        }

        let mut emit_by_field: BTreeMap<String, FieldEmit> = BTreeMap::new();
        for row in &facts.field_emits {
            let (Some(f), Some(e)) = (str_at(row, "FieldId"), str_at(row, "EmitType")) else {
                continue;
            };
            emit_by_field.insert(f, FieldEmit::parse(&e));
        }

        // Every panel/3 row, parts included, keyed by (namespace, id). A
        // republished panel can leave identical registration rows behind; the
        // first one wins and the rest collapse.
        let mut flat: Vec<Panel> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for row in &facts.panels {
            let (Some(id), Some(kind), Some(ns)) = (
                str_at(row, "Id"),
                str_at(row, "Kind"),
                str_at(row, "Namespace"),
            ) else {
                continue;
            };
            if !seen.insert((ns.clone(), id.clone())) {
                continue;
            }
            let props = props_by_id.get(&id).cloned().unwrap_or_default();
            flat.push(Panel {
                kind: PanelKind::parse(&kind),
                namespace: ns,
                rows: data_by_id.get(&id).cloned().unwrap_or_default(),
                source: source_by_id.get(&id).cloned(),
                published: prop_bool(&props, "published").unwrap_or(false),
                props,
                fields: Vec::new(),
                children: Vec::new(),
                id,
            });
        }

        let field_meta = FieldMeta {
            inputs: &inputs_by_field,
            triggers: &trigger_by_field,
            emits: &emit_by_field,
        };
        fold_parts(flat, &field_meta)
    }

    /// Build a single panel by id, or `None` if it is absent.
    pub fn from_facts(facts: &PanelFacts, id: &str) -> Option<Panel> {
        Self::all_from_facts(facts).into_iter().find(|p| p.id == id)
    }

    /// Build every panel from a `smart_panel.list` response.
    ///
    /// Accepts either the whole response (`{"panels": [...]}`) or the bare
    /// array. Each entry carries `id`, `kind`, `namespace`, `props`, and
    /// optionally `data_preview`, `source_info` and `field_ids`.
    ///
    /// **Rows are a preview.** `data_preview` holds at most the first few rows;
    /// a renderer that needs the full set still reads `panel_data` or runs the
    /// live source. Field-level metadata (`field_input` / `field_trigger` /
    /// `field_emit`) is not part of the list output and comes back empty.
    pub fn all_from_list(response: &Value) -> Vec<Panel> {
        let entries = response
            .get("panels")
            .and_then(Value::as_array)
            .or_else(|| response.as_array())
            .cloned()
            .unwrap_or_default();

        let mut flat: Vec<Panel> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for entry in &entries {
            let (Some(id), Some(kind), Some(ns)) = (
                str_at(entry, "id"),
                str_at(entry, "kind"),
                str_at(entry, "namespace"),
            ) else {
                continue;
            };
            if !seen.insert((ns.clone(), id.clone())) {
                continue;
            }

            let props: Props = entry
                .get("props")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();

            let source = entry.get("source_info").and_then(|s| {
                Some(PanelSource {
                    namespace: str_at(s, "namespace")?,
                    query: str_at(s, "query")?,
                })
            });

            flat.push(Panel {
                kind: PanelKind::parse(&kind),
                namespace: ns,
                rows: entry
                    .get("data_preview")
                    .map(normalize_rows)
                    .unwrap_or_default(),
                source,
                published: prop_bool(&props, "published").unwrap_or(false),
                props,
                fields: Vec::new(),
                children: Vec::new(),
                id,
            });
        }

        let empty = BTreeMap::new();
        let empty_t = BTreeMap::new();
        let empty_e = BTreeMap::new();
        let field_meta = FieldMeta {
            inputs: &empty,
            triggers: &empty_t,
            emits: &empty_e,
        };
        fold_parts(flat, &field_meta)
    }
}

struct FieldMeta<'a> {
    inputs: &'a BTreeMap<String, Vec<String>>,
    triggers: &'a BTreeMap<String, FieldTrigger>,
    emits: &'a BTreeMap<String, FieldEmit>,
}

/// Move every panel that names a `parent` under that parent.
///
/// A part of a **form** becomes a [`Field`]; a part of anything else (a
/// dashboard, in practice) becomes a child [`Panel`], and is itself folded
/// first so nested containers resolve. A part whose parent is not in the set
/// stays at the top level rather than vanishing — better an orphan you can see
/// than a panel that silently disappears.
fn fold_parts(flat: Vec<Panel>, meta: &FieldMeta<'_>) -> Vec<Panel> {
    // Index parts by their parent id. Parents are named by id alone (the
    // `parent` prop carries no namespace), so resolve by id.
    let ids: BTreeSet<String> = flat.iter().map(|p| p.id.clone()).collect();
    let mut by_parent: BTreeMap<String, Vec<Panel>> = BTreeMap::new();
    let mut roots: Vec<Panel> = Vec::new();

    for panel in flat {
        match panel.parent() {
            // A self-parent is a publishing mistake; treating it as a root
            // avoids an infinite fold.
            Some(parent) if parent != panel.id && ids.contains(&parent) => {
                by_parent.entry(parent).or_default().push(panel);
            }
            _ => roots.push(panel),
        }
    }

    let mut visiting = BTreeSet::new();
    roots
        .into_iter()
        .map(|root| attach_parts(root, &mut by_parent, meta, &mut visiting))
        .collect()
}

fn attach_parts(
    mut panel: Panel,
    by_parent: &mut BTreeMap<String, Vec<Panel>>,
    meta: &FieldMeta<'_>,
    visiting: &mut BTreeSet<String>,
) -> Panel {
    // Cycle guard: a→b→a would otherwise recurse forever. The second visit
    // simply gets no parts.
    if !visiting.insert(panel.id.clone()) {
        return panel;
    }

    if let Some(parts) = by_parent.remove(&panel.id) {
        if panel.kind.is_form_kind() {
            panel.fields = parts
                .into_iter()
                .map(|part| Field {
                    inputs: meta.inputs.get(&part.id).cloned().unwrap_or_default(),
                    trigger: meta.triggers.get(&part.id).cloned(),
                    emit: meta.emits.get(&part.id).cloned(),
                    id: part.id,
                    kind: part.kind,
                    props: part.props,
                    source: part.source,
                })
                .collect();
        } else {
            panel.children = parts
                .into_iter()
                .map(|part| attach_parts(part, by_parent, meta, visiting))
                .collect();
        }
    }

    visiting.remove(&panel.id);
    panel
}

/// Read a string field from a solution row, coercing numbers and bools.
///
/// Prolog atoms usually arrive as JSON strings but numeric values can arrive
/// as numbers; treating those as absent would silently drop data.
fn str_at(row: &Value, key: &str) -> Option<String> {
    match row.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Flatten the three row shapes into `Vec<Vec<Value>>`.
///
/// - `[[a, b], [c, d]]` → as-is
/// - `[{"month": "Jan", "amt": 4}, ...]` → values in stable key order
/// - `"{amount:420,claim:clm_501}"` → parsed as a curly term
pub fn normalize_rows(rows: &Value) -> Vec<Vec<Value>> {
    let Some(list) = rows.as_array() else {
        return Vec::new();
    };

    list.iter()
        .map(|row| match row {
            Value::Array(cells) => cells.clone(),
            Value::Object(map) => map.values().cloned().collect(),
            Value::String(s) if s.starts_with('{') => parse_curly_term(s)
                .into_iter()
                .map(|(_, v)| Value::String(v))
                .collect(),
            other => vec![other.clone()],
        })
        .collect()
}

/// Parse a Prolog-rendered curly term — `"{amount:420,claim:'high risk'}"` —
/// into ordered key/value pairs.
///
/// Not a general Prolog parser: it tracks quote and brace depth so that commas
/// and colons inside quoted values or nested braces do not split the term. That
/// is enough for what `panel_data` actually emits.
pub fn parse_curly_term(s: &str) -> Vec<(String, String)> {
    let inner = s.trim().trim_start_matches('{').trim_end_matches('}');
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut current = String::new();

    for ch in inner.chars() {
        match ch {
            '\'' | '"' if quote == Some(ch) => {
                quote = None;
                current.push(ch);
            }
            '\'' | '"' if quote.is_none() => {
                quote = Some(ch);
                current.push(ch);
            }
            '{' | '[' if quote.is_none() => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' if quote.is_none() => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if quote.is_none() && depth == 0 => {
                push_pair(&mut out, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_pair(&mut out, &current);
    out
}

fn push_pair(out: &mut Vec<(String, String)>, raw: &str) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    match raw.split_once(':') {
        Some((k, v)) => out.push((k.trim().to_string(), unquote(v.trim()))),
        None => out.push((String::new(), unquote(raw))),
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0] as char;
        if (first == '\'' || first == '"') && bytes[s.len() - 1] as char == first {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn panel(id: &str, kind: &str, ns: &str) -> Value {
        json!({"Id": id, "Kind": kind, "Namespace": ns})
    }
    fn prop(id: &str, key: &str, value: Value) -> Value {
        json!({"Id": id, "Key": key, "Value": value})
    }

    #[test]
    fn parses_a_panel_with_props_and_source() {
        let facts = PanelFacts {
            panels: vec![panel("rev", "bar_chart", "app")],
            props: vec![
                prop("rev", "title", json!("Revenue")),
                prop("rev", "slot", json!("side")),
                prop("rev", "published", json!(true)),
            ],
            sources: vec![json!({"Id": "rev", "Ns": "app", "Query": "monthly(M, A)"})],
            ..Default::default()
        };

        let panels = Panel::all_from_facts(&facts);
        assert_eq!(panels.len(), 1);
        let p = &panels[0];
        assert_eq!(p.title(), "Revenue");
        assert_eq!(p.slot(), "side");
        assert_eq!(p.kind, PanelKind::BarChart);
        assert!(p.published);
        assert_eq!(p.source.as_ref().unwrap().query, "monthly(M, A)");
    }

    #[test]
    fn published_must_be_declared_true_like_the_server() {
        let facts = PanelFacts {
            panels: vec![
                panel("a", "metric", "ns"),
                panel("b", "metric", "ns"),
                panel("c", "metric", "ns"),
            ],
            props: vec![
                prop("a", "published", json!(true)),
                prop("b", "published", json!("true")),
                // c declares nothing.
            ],
            ..Default::default()
        };
        let by_id: BTreeMap<_, _> = Panel::all_from_facts(&facts)
            .into_iter()
            .map(|p| (p.id.clone(), p.published))
            .collect();
        assert!(by_id["a"]);
        assert!(by_id["b"]);
        assert!(!by_id["c"], "absent means unpublished, as on the server");
    }

    #[test]
    fn folds_fields_into_their_form_via_the_parent_prop() {
        let facts = PanelFacts {
            panels: vec![
                panel("contact", "form", "app"),
                panel("company", "text_input", "app"),
            ],
            props: vec![
                prop("company", "parent", json!("contact")),
                prop("company", "label", json!("Company Name")),
                prop("company", "required", json!(true)),
            ],
            field_triggers: vec![
                json!({"FieldId": "company", "Type": "on_event", "Event": "change"}),
            ],
            field_emits: vec![json!({"FieldId": "company", "EmitType": "replacement"})],
            ..Default::default()
        };

        let panels = Panel::all_from_facts(&facts);
        // The field must not surface as a top-level panel.
        assert_eq!(panels.len(), 1);
        let form = &panels[0];
        assert_eq!(form.fields.len(), 1);
        let field = &form.fields[0];
        assert_eq!(field.label(), "Company Name");
        assert!(field.required());

        let trigger = field.trigger.as_ref().unwrap();
        assert_eq!(trigger.trigger_type, TriggerType::OnEvent);
        assert_eq!(trigger.event, TriggerEvent::Change);
        assert!(field.fires_on(&TriggerEvent::Change));
        assert!(!field.fires_on(&TriggerEvent::Submit));
        assert_eq!(field.emit, Some(FieldEmit::Replacement));
    }

    #[test]
    fn an_unknown_trigger_or_emit_is_carried_through_not_dropped() {
        let facts = PanelFacts {
            panels: vec![
                panel("contact", "form", "app"),
                panel("company", "text_input", "app"),
            ],
            props: vec![prop("company", "parent", json!("contact"))],
            field_triggers: vec![
                json!({"FieldId": "company", "Type": "on_quantum", "Event": "hover"}),
            ],
            field_emits: vec![json!({"FieldId": "company", "EmitType": "teleport"})],
            ..Default::default()
        };

        let field = &Panel::all_from_facts(&facts)[0].fields[0];
        let trigger = field.trigger.as_ref().unwrap();
        // A vocabulary this crate predates must survive the round trip, the
        // same way an unrecognized kind does.
        assert_eq!(
            trigger.trigger_type,
            TriggerType::Unknown("on_quantum".into())
        );
        assert_eq!(trigger.trigger_type.as_str(), "on_quantum");
        assert_eq!(trigger.event.as_str(), "hover");
        assert_eq!(field.emit.as_ref().unwrap().as_str(), "teleport");
    }

    #[test]
    fn a_field_with_no_trigger_fires_nothing_of_its_own() {
        let facts = PanelFacts {
            panels: vec![
                panel("contact", "form", "app"),
                panel("company", "text_input", "app"),
            ],
            props: vec![prop("company", "parent", json!("contact"))],
            ..Default::default()
        };

        let field = &Panel::all_from_facts(&facts)[0].fields[0];
        assert!(field.trigger.is_none());
        // Its value travels with the form's submit instead.
        assert!(!field.fires_on(&TriggerEvent::Submit));
        assert!(!field.fires_on(&TriggerEvent::Change));
    }

    #[test]
    fn folds_dashboard_children_via_the_parent_prop() {
        let facts = PanelFacts {
            panels: vec![
                panel("board", "dashboard", "pulse"),
                panel("total", "metric", "pulse"),
                panel("mix", "bar_chart", "pulse"),
                panel("unrelated", "table", "other"),
            ],
            props: vec![
                prop("board", "title", json!("Pulse")),
                prop("total", "parent", json!("board")),
                prop("mix", "parent", json!("board")),
            ],
            data: vec![json!({"Id": "total", "Rows": [["Total", 42]]})],
            ..Default::default()
        };

        let panels = Panel::all_from_facts(&facts);
        assert_eq!(panels.len(), 2, "board and the unrelated table only");

        let board = panels.iter().find(|p| p.id == "board").unwrap();
        assert_eq!(board.kind, PanelKind::Dashboard);
        assert_eq!(board.children.len(), 2);
        assert!(
            board.fields.is_empty(),
            "dashboard parts are panels, not fields"
        );

        // Children keep their own rows.
        let total = board.children.iter().find(|c| c.id == "total").unwrap();
        assert_eq!(total.rows, vec![vec![json!("Total"), json!(42)]]);
    }

    #[test]
    fn duplicate_registrations_and_parent_edges_collapse() {
        // Republishing leaves identical panel/3 rows and repeated parent
        // facts behind. The result must be one panel with each child once.
        let facts = PanelFacts {
            panels: vec![
                panel("probe", "bar_chart", "crm"),
                panel("probe", "bar_chart", "crm"),
                panel("probe", "bar_chart", "crm"),
                panel("board", "dashboard", "crm"),
                panel("kid", "metric", "crm"),
                panel("kid", "metric", "crm"),
            ],
            props: vec![
                prop("kid", "parent", json!("board")),
                prop("kid", "parent", json!("board")),
            ],
            ..Default::default()
        };

        let panels = Panel::all_from_facts(&facts);
        assert_eq!(panels.iter().filter(|p| p.id == "probe").count(), 1);
        let board = panels.iter().find(|p| p.id == "board").unwrap();
        assert_eq!(board.children.len(), 1);
    }

    #[test]
    fn same_id_in_different_namespaces_are_different_panels() {
        let facts = PanelFacts {
            panels: vec![
                panel("total", "metric", "sales"),
                panel("total", "metric", "support"),
            ],
            ..Default::default()
        };
        assert_eq!(Panel::all_from_facts(&facts).len(), 2);
    }

    #[test]
    fn a_part_whose_parent_is_missing_stays_visible() {
        let facts = PanelFacts {
            panels: vec![panel("orphan", "metric", "ns")],
            props: vec![prop("orphan", "parent", json!("gone"))],
            ..Default::default()
        };
        // Better an orphan you can see than a panel that silently disappears.
        assert_eq!(Panel::all_from_facts(&facts).len(), 1);
    }

    #[test]
    fn a_parent_cycle_does_not_recurse_forever() {
        let facts = PanelFacts {
            panels: vec![
                panel("a", "dashboard", "ns"),
                panel("b", "dashboard", "ns"),
                panel("c", "metric", "ns"),
            ],
            props: vec![
                prop("a", "parent", json!("b")),
                prop("b", "parent", json!("a")),
                prop("c", "parent", json!("c")), // self-parent
            ],
            ..Default::default()
        };
        let panels = Panel::all_from_facts(&facts);
        // Everything is reachable, nothing hangs.
        assert!(!panels.is_empty());
        assert!(panels.iter().any(|p| p.id == "c"));
    }

    #[test]
    fn list_typed_props_survive() {
        let facts = PanelFacts {
            panels: vec![panel("overdue", "table", "ar")],
            props: vec![
                prop(
                    "overdue",
                    "columns",
                    json!(["Invoice", "Customer", "Days", "Amount"]),
                ),
                prop("overdue", "published", json!(true)),
            ],
            data: vec![json!({"Id": "overdue", "Rows": [["INV-1", "Acme", 31, 1200.5]]})],
            ..Default::default()
        };
        let p = Panel::all_from_facts(&facts).remove(0);
        assert_eq!(p.columns(), vec!["Invoice", "Customer", "Days", "Amount"]);
        assert_eq!(p.rows[0].len(), 4);
    }

    #[test]
    fn unknown_kind_round_trips_rather_than_panicking() {
        assert_eq!(
            PanelKind::parse("sonar_display"),
            PanelKind::Unknown("sonar_display".into())
        );
    }

    #[test]
    fn normalizes_the_three_row_shapes() {
        let arrays = normalize_rows(&json!([["Jan", 4], ["Feb", 6]]));
        assert_eq!(arrays[1][0], json!("Feb"));

        let objects = normalize_rows(&json!([{"a": 1, "b": 2}]));
        assert_eq!(objects[0].len(), 2);

        let curly = normalize_rows(&json!(["{amount:420,claim:clm_501}"]));
        assert_eq!(curly[0], vec![json!("420"), json!("clm_501")]);
    }

    #[test]
    fn curly_term_keeps_commas_inside_quotes() {
        let pairs = parse_curly_term("{reason:'high risk, escalated',amount:420}");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].1, "high risk, escalated");
        assert_eq!(pairs[1].1, "420");
    }

    // ── smart_panel.list input ───────────────────────────────────────────

    fn list_response() -> Value {
        json!({
            "panels": [
                {
                    "id": "board", "kind": "dashboard", "namespace": "pulse",
                    "props": {"title": "Pulse", "published": true},
                    "has_data": false, "has_source": false,
                    "field_count": 2, "field_ids": ["total", "mix"],
                    "data_preview": [], "source_info": null
                },
                {
                    "id": "total", "kind": "metric", "namespace": "pulse",
                    "props": {"parent": "board", "published": true},
                    "has_data": true, "has_source": false,
                    "field_count": 0, "field_ids": [],
                    "data_preview": [["Total", 42]], "source_info": null
                },
                {
                    "id": "mix", "kind": "bar_chart", "namespace": "pulse",
                    "props": {"parent": "board", "published": true, "columns": ["Stage", "Count"]},
                    "has_data": false, "has_source": true,
                    "field_count": 0, "field_ids": [],
                    "data_preview": [],
                    "source_info": {"namespace": "pulse", "query": "stage_count(S, N)"}
                }
            ],
            "total": 3,
            "message": "3 panels"
        })
    }

    #[test]
    fn parses_a_smart_panel_list_response() {
        let panels = Panel::all_from_list(&list_response());
        assert_eq!(panels.len(), 1, "children fold under the dashboard");

        let board = &panels[0];
        assert_eq!(board.kind, PanelKind::Dashboard);
        assert_eq!(board.title(), "Pulse");
        assert!(board.published);
        assert_eq!(board.children.len(), 2);

        let mix = board.children.iter().find(|c| c.id == "mix").unwrap();
        assert_eq!(mix.columns(), vec!["Stage", "Count"]);
        assert_eq!(mix.source.as_ref().unwrap().query, "stage_count(S, N)");

        let total = board.children.iter().find(|c| c.id == "total").unwrap();
        assert_eq!(total.rows, vec![vec![json!("Total"), json!(42)]]);
    }

    #[test]
    fn list_input_accepts_a_bare_array_too() {
        let bare = list_response()["panels"].clone();
        assert_eq!(Panel::all_from_list(&bare).len(), 1);
    }
}
