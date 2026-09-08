# datagrout-panels

The [DataGrout](https://datagrout.ai) **Smart Panel** model: declarative UI
stored as logic-cell facts, parsed into a renderer-agnostic `Panel` tree.

A Smart Panel is a set of Prolog facts in a cell's `_panels` namespace:

```prolog
panel(revenue_chart, bar_chart, my_app).
panel_prop(revenue_chart, title, 'Revenue by Month').
panel_source(revenue_chart, my_app, 'monthly_revenue(Month, Amt)').
```

That makes a panel queryable, composable and versioned like any other
knowledge in the cell, and **derived** rather than stored: its `panel_source`
goal re-runs against the rulebase every time the panel is read.

This crate parses those facts and stops. Rendering lives in separate crates —
[`datagrout-panels-egui`](https://crates.io/crates/datagrout-panels-egui)
(native GUI) and
[`datagrout-panels-mcp`](https://crates.io/crates/datagrout-panels-mcp)
(MCP Apps) — so a consumer that only transpiles never links a GUI toolkit.

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
