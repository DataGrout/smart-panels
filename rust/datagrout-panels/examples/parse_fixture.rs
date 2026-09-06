//! Parse a captured `_panels` fact dump and report what came out.
//!
//! ```bash
//! cargo run -p datagrout-panels --example parse_fixture -- path/to/facts.json
//! ```
//!
//! The file holds one array of `logic.query` solution rows per goal, keyed
//! `panels`, `props`, `data`, `sources`, `field_inputs`, `field_triggers`,
//! `field_emits` (any may be absent). This is how the parser gets checked
//! against facts a real gateway produced, rather than against fixtures written
//! from reading the schema — the two differ in exactly the ways that matter.

use std::collections::BTreeMap;

use datagrout_panels::{Panel, PanelFacts};
use serde_json::Value;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: parse_fixture <facts.json>");
    let raw = std::fs::read_to_string(&path).expect("read fixture");
    let doc: BTreeMap<String, Value> =
        serde_json::from_str(&raw).expect("fixture is a JSON object");

    let rows = |key: &str| -> Vec<Value> {
        doc.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };

    let facts = PanelFacts {
        panels: rows("panels"),
        props: rows("props"),
        data: rows("data"),
        sources: rows("sources"),
        field_inputs: rows("field_inputs"),
        field_triggers: rows("field_triggers"),
        field_emits: rows("field_emits"),
    };

    println!(
        "input rows: panels={} props={} data={} sources={} fields(in/trig/emit)={}/{}/{}",
        facts.panels.len(),
        facts.props.len(),
        facts.data.len(),
        facts.sources.len(),
        facts.field_inputs.len(),
        facts.field_triggers.len(),
        facts.field_emits.len()
    );

    let panels = Panel::all_from_facts(&facts);
    println!("parsed panels: {}\n", panels.len());

    let mut unknown = 0;
    for p in &panels {
        let kind = p.kind.as_str().to_string();
        if matches!(p.kind, datagrout_panels::PanelKind::Unknown(_)) {
            unknown += 1;
        }
        println!(
            "{:<24} {:<11} ns={:<20} props={:<2} rows={:<3} cols={:<2} source={} fields={} children={} published={}",
            p.id,
            kind,
            p.namespace,
            p.props.len(),
            p.rows.len(),
            p.rows.first().map(Vec::len).unwrap_or(0),
            p.source.is_some(),
            p.fields.len(),
            p.children.len(),
            p.published
        );
        for c in &p.children {
            println!(
                "    └ {:<20} {:<11} rows={:<3} columns={:?}",
                c.id,
                c.kind.as_str(),
                c.rows.len(),
                c.columns()
            );
        }
    }

    // Anything a renderer would fall back on is worth calling out by name.
    println!("\nunknown kinds: {unknown}");
    let titled = panels
        .iter()
        .filter(|p| p.props.contains_key("title"))
        .count();
    println!("panels with a title prop: {titled}/{}", panels.len());
}
