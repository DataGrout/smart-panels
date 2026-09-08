# datagrout-panels

[![crates.io](https://img.shields.io/crates/v/datagrout-panels.svg)](https://crates.io/crates/datagrout-panels)
[![docs.rs](https://img.shields.io/docsrs/datagrout-panels)](https://docs.rs/datagrout-panels)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The [DataGrout](https://datagrout.ai) **Smart Panel** model: declarative UI
stored as logic-cell facts, parsed into a renderer-agnostic `Panel` tree.

A panel is created on DataGrout by calling the gateway's `smart_panel.publish`
tool:

```json
{
  "id": "revenue_chart",
  "kind": "bar_chart",
  "namespace": "my_app",
  "props": { "title": "Revenue by Month" },
  "source": { "namespace": "my_app", "query": "monthly_revenue(Month, Amt)" }
}
```

which compiles into Prolog facts in the `_panels` namespace of a logic cell —
the same knowledge base the rest of your rules and data live in:

```prolog
panel(revenue_chart, bar_chart, my_app).
panel_prop(revenue_chart, title, 'Revenue by Month').
panel_source(revenue_chart, my_app, 'monthly_revenue(Month, Amt)').
```

`panel/3` is the id, the kind and the owning namespace; `panel_prop/3` is one
fact per config key; `panel_source/3` names a namespace and a **Prolog goal**
whose solutions are the panel's rows — one row per solution, `Month` and `Amt`
as the columns. (A panel with fixed rows instead carries `panel_data/2`.)

Nothing is copied into the panel and the goal re-runs on every read, so a panel
is **derived** rather than stored.

That is what the "smart" is doing. The goal can call *rules*, not just match
stored facts, so a panel over `at_risk(Deal)` shows whatever satisfies that
rule right now: change the rule in the cell and every panel built on it
changes, with no panel edited and no cache to invalidate. Because the
definitions are themselves facts, an agent holding `smart_panel.publish` can
build a dashboard as an outcome of its reasoning, and `logic.query` can audit
what exists. Form fields go further — they declare dependencies, triggers and
emits, and a field's goal can invoke a tool and replace the field's own value.

This crate turns those facts into something a renderer can walk.

This crate parses those facts and stops. Rendering lives in separate crates —
[`datagrout-panels-egui`](https://crates.io/crates/datagrout-panels-egui)
(native GUI) and
[`datagrout-panels-mcp`](https://crates.io/crates/datagrout-panels-mcp)
(MCP Apps) — so a consumer that only transpiles never links a GUI toolkit.

## Where panels live, and how you read them

A logic cell is scoped to one account **and one hub server**, so the panels you
can read are those published through the server you connected to. Within
`_panels`, each panel's own `namespace` (`my_app` above) groups it with its
siblings.

Composites are facts too, not nesting: a **dashboard** is published as
`kind: "dashboard"` with each child naming it in `props.parent`, and a **form**
as `kind: "form"` with a `fields` array whose entries may declare `inputs`, a
`trigger` and an `emit`. The parser inverts those edges into `children` and
`fields`.

Reading panels back is a single `smart_panel.list` call, whose response is what
`Panel::all_from_list` parses. **There is no transport here, and this crate
creates nothing**: bring an MCP client such as
[conduit-sdk](https://github.com/DataGrout/conduit-sdk), or any MCP-speaking
host, and hand the response over.

Kinds and props are a fixed vocabulary — the display kinds (`bar_chart`,
`table`, `metric`, `gauge`, `dashboard`, …), the form kinds (`form`,
`text_input`, `dropdown`, `button`, …), and props like `title`, `parent`,
`slot`, `columns` and `published`. The full list, with prop types, is in
[`SPEC.md`](https://github.com/DataGrout/smart-panels/blob/main/SPEC.md).

No account yet? The parser takes plain JSON, so a hand-written list response
renders like a real one:

```rust
use datagrout_panels::Panel;
use serde_json::json;

let panels = Panel::all_from_list(&json!({
    "panels": [{
        "id": "revenue", "kind": "bar_chart", "namespace": "demo",
        "props": { "title": "Revenue by Month", "columns": ["Month", "Amount"] },
        "data_preview": [["Jan", 12500], ["Feb", 18300], ["Mar", 21100]]
    }]
}));

assert_eq!(panels[0].title(), "Revenue by Month");
assert_eq!(panels[0].rows.len(), 3);
```

## Two ways in

**From `smart_panel.list`** — the intended path. One gateway call returns every
panel with its props, a row preview and its parts already resolved:

```rust
use datagrout_panels::Panel;

// `response` is the JSON the `smart_panel.list` tool returned.
let panels: Vec<Panel> = Panel::all_from_list(&response);
for p in &panels {
    println!("{} [{}] {} children", p.title(), p.kind.as_str(), p.children.len());
}
```

**From raw facts** — when you need full row sets or field metadata. Run each
goal in `goals` through `logic.query` and hand the solution rows over:

```rust
use datagrout_panels::{goals, Panel, PanelFacts};

let facts = PanelFacts {
    panels: query(goals::PANELS),        // your logic.query call
    props: query(goals::PROPS),
    data: query(goals::DATA),
    sources: query(goals::SOURCE),
    ..Default::default()
};
let panels = Panel::all_from_facts(&facts);
```

## The model

`Panel { id, kind, namespace, props, rows, source, fields, children, published }`.

- **Kinds** mirror the gateway's publish tool: charts, `table`, `metric`,
  `gauge`, `markdown`, `dashboard`, and the form kinds (`form`, `text_input`,
  `dropdown`, `button`, …). A kind this crate predates parses as
  `PanelKind::Unknown(String)` so a renderer can draw a placeholder instead of
  failing.
- **Composition** is by the child's `parent` prop. A dashboard's children and
  a form's fields are their own `panel/3` facts naming the container; the
  parser inverts that edge into `children` and `fields`, and parts do not
  appear at the top level.
- **Props are typed.** `columns` is a list, `published` a boolean. `props` is a
  `BTreeMap<String, serde_json::Value>` with coercing accessors: `prop_str`,
  `prop_list`, `prop_bool`, `prop_f64`, plus `title()`, `description()`,
  `columns()`, `parent()`, `slot()`.
- **Rows** are `Vec<Vec<Value>>` after `normalize_rows`, which accepts the
  list-of-lists snapshot form and the object-per-solution form `logic.query`
  returns.
- **Identity** is `(namespace, id)`; duplicate registrations from a republish
  collapse to one.

## What a fetch layer has to know

This crate does not fetch. Whatever does should expect:

- An undefined predicate is an *error* from `logic.query`, not an empty result.
  Treat it as empty.
- Results over roughly 48 KB come back as a `cache_ref` rather than inline;
  `prism.paginate` pages them, as solution rows.
- `panel_data(Id, Rows)` is an ordered snapshot; `panel_source` rows come back
  as objects whose key order is not the query's variable order. Prefer the
  snapshot when both exist.

The full contract, including renderer conformance, is in
[`SPEC.md`](https://github.com/DataGrout/smart-panels/blob/main/SPEC.md).

## Security

Panel facts may have been asserted by an agent. Treat every prop and row as
untrusted text; the renderers escape accordingly.

## License

`MIT OR Apache-2.0`, at your option.
