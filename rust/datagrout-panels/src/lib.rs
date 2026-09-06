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
//! Which makes a panel definition queryable, composable, and versioned like any
//! other knowledge in the cell — and means a panel is *derived*, not stored: its
//! `panel_source` goal is re-run against the rulebase every time it is read.
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
//! ```no_run
//! use datagrout_panels::Panel;
//!
//! # fn demo(list_response: serde_json::Value) {
//! let panels = Panel::all_from_list(&list_response);
//! for panel in &panels {
//!     println!("{} ({}) — {} children", panel.title(), panel.kind.as_str(), panel.children.len());
//! }
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod facts;
pub mod model;

pub use facts::{goals, normalize_rows, parse_curly_term, PanelFacts};
pub use model::{
    prop_bool, prop_f64, prop_list, prop_str, Field, FieldEmit, FieldTrigger, Panel, PanelKind,
    PanelSource, Props,
};

/// The system namespace every Smart Panel is published into.
pub const PANELS_NAMESPACE: &str = "_panels";
