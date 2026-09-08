//! The DataGrout Smart Panel model.
//!
//! A Smart Panel is not a JSON blob — it is a set of Prolog facts living in a
//! logic cell's `_panels` namespace:
//!
//! ```prolog
//! panel(revenue_chart, bar_chart, my_app).
//! panel_prop(revenue_chart, title, 'Revenue by Month').
//! panel_source(revenue_chart, my_app, 'monthly_revenue(Month, Amt)').
//! ```
//!
//! `panel/3` carries the id, the kind and the owning namespace; `panel_prop/3`
//! is one fact per config key; `panel_source/3` names a namespace and a
//! **Prolog goal** whose solutions are the panel's rows — one row per solution,
//! `Month` and `Amt` as the columns. A panel with fixed rows instead carries
//! `panel_data/2`.
//!
//! Which makes a panel definition queryable, composable, and versioned like any
//! other knowledge in the cell — and means a panel is *derived*, not stored: its
//! `panel_source` goal is re-run against the rulebase every time it is read.
//!
//! # Why "smart"
//!
//! The goal can call *rules*, not just match stored facts, so a panel over
//! `at_risk(Deal)` shows whatever satisfies that rule at read time: change the
//! rule and every panel built on it changes, with no panel edited and no cache
//! to invalidate. Because the definitions are facts, an agent can publish a
//! dashboard as an outcome of its reasoning, and `logic.query` can audit what
//! exists. Form fields carry dependency edges, triggers and emits, and a
//! field's goal may invoke a tool and replace the field's own value — a small
//! dataflow graph that runs *in the cell*: on submit, DataGrout binds the
//! fields into the panel's goal (or into the `+` inputs of a rule published
//! with `reactor.expose`) and evaluates it under the cell's sandbox. A host
//! chooses where to submit; it does not implement the cascade.
//!
//! # This crate is the model, not a renderer
//!
//! It parses facts into a [`Panel`] tree and stops. Rendering lives in separate
//! crates so that one panel definition can drive several very different
//! surfaces:
//!
//! | crate | surface |
//! |---|---|
//! | `datagrout-panels-egui` | native immediate-mode GUI |
//! | `datagrout-panels-mcp` | MCP Apps (SEP-1865) `ui://` resources |
//!
//! Keeping the model renderer-free is what makes that possible, and it means a
//! consumer that only transpiles never links a GUI toolkit.
//!
//! # Where panels come from
//!
//! This crate does not create panels and does not fetch them. A Smart Panel is
//! created on DataGrout by calling the gateway's `smart_panel.publish` tool
//! with an id, a kind, an owning namespace, and whatever props, rows or backing
//! query it needs; a dashboard is published as `kind: "dashboard"` with each
//! child naming it in `props.parent`, and a form as `kind: "form"` with a
//! `fields` array.
//!
//! The resulting facts live in the `_panels` namespace of a logic cell, and a
//! cell is scoped to one account **and one hub server** — so the panels a
//! caller can read are those published through the server it connected to.
//! Reading them back is a single `smart_panel.list` call, and its response is
//! what [`Panel::all_from_list`] takes. Bring your own MCP client.
//!
//! # Two ways in
//!
//! * [`Panel::all_from_list`] — feed it a `smart_panel.list` response. The
//!   intended path: one call, props included, children resolved.
//! * [`Panel::all_from_facts`] — feed it raw `logic.query` rows per goal (see
//!   [`goals`]) when you need full rows or field metadata.
//!
//! Nothing here fetches. See [`facts`] for the fetch-layer facts a caller has
//! to know — undefined predicates, result paging, duplicate registrations.
//!
//! A hand-written list response parses like a real one, which is how to see a
//! renderer work before you have panels of your own:
//!
//! ```
//! use datagrout_panels::Panel;
//! use serde_json::json;
//!
//! let panels = Panel::all_from_list(&json!({
//!     "panels": [{
//!         "id": "revenue", "kind": "bar_chart", "namespace": "demo",
//!         "props": { "title": "Revenue by Month", "columns": ["Month", "Amount"] },
//!         "data_preview": [["Jan", 12500], ["Feb", 18300], ["Mar", 21100]]
//!     }]
//! }));
//!
//! assert_eq!(panels[0].title(), "Revenue by Month");
//! assert_eq!(panels[0].columns(), ["Month", "Amount"]);
//! assert_eq!(panels[0].rows.len(), 3);
//! ```

#![forbid(unsafe_code)]

pub mod facts;
pub mod model;

pub use facts::{goals, normalize_rows, parse_curly_term, PanelFacts};
pub use model::{
    prop_bool, prop_f64, prop_list, prop_str, Field, FieldEmit, FieldTrigger, Panel, PanelKind,
    PanelSource, Props, TriggerEvent, TriggerType,
};

/// The system namespace every Smart Panel is published into.
pub const PANELS_NAMESPACE: &str = "_panels";
