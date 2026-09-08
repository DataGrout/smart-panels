//! Render [DataGrout](https://datagrout.ai) Smart Panels with
//! [egui](https://github.com/emilk/egui).
//!
//! # Why immediate mode
//!
//! egui redraws from state every frame with no retained widget tree. A Smart
//! Panel re-derives its `panel_source` from the rulebase on every load, with no
//! retained DOM. They are the same architecture — so rendering one with the
//! other costs no reconciliation, unlike a renderer that must diff panel facts
//! into a persistent element tree.
//!
//! One function per kind, dispatched by [`render_panel`]. Every renderer is
//! pure: it reads the panel and draws. Nothing here fetches, caches, or mutates.
//!
//! Form fields report interaction through [`PanelAction`] rather than acting on
//! it, so the *caller* decides what a submit means. This crate has no transport
//! and must not grow one.
//!
//! ```no_run
//! use datagrout_panels::Panel;
//! use datagrout_panels_egui::render_panel;
//!
//! # fn demo(ctx: &egui::Context, list_response: serde_json::Value) {
//! let panels = Panel::all_from_list(&list_response);
//!
//! egui::CentralPanel::default().show(ctx, |ui| {
//!     for panel in &panels {
//!         render_panel(ui, panel);
//!     }
//! });
//! # }
//! ```

#![forbid(unsafe_code)]

use egui::{Color32, RichText, Sense, Stroke, Ui, Vec2};
use serde_json::Value;

use datagrout_panels::{Panel, PanelKind};

/// Something a viewer did that the host application must act on.
///
/// Returned rather than executed: dispatching a field's `panel_source` goal
/// requires a transport, and this crate deliberately has none — the same panel
/// must be renderable by a host that reaches its cell over MCP, over a local
/// proxy, or not at all.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// A `button` field or form submit fired.
    Submit { panel_id: String, field_id: String },
    /// A field's value changed. Cascade to dependents via `field_input` edges.
    ValueChanged {
        panel_id: String,
        field_id: String,
        value: String,
    },
}

/// Mutable per-viewer form state. Kept by the caller across frames — immediate
/// mode means the widgets themselves hold nothing.
#[derive(Debug, Default, Clone)]
pub struct FormState {
    pub values: std::collections::BTreeMap<String, String>,
    pub checks: std::collections::BTreeMap<String, bool>,
}

/// Render a panel. Returns any actions the viewer triggered this frame.
pub fn render_panel(ui: &mut Ui, panel: &Panel) -> Vec<PanelAction> {
    let mut state = FormState::default();
    render_panel_with_state(ui, panel, &mut state)
}

/// Render a panel with caller-held form state.
pub fn render_panel_with_state(
    ui: &mut Ui,
    panel: &Panel,
    state: &mut FormState,
) -> Vec<PanelAction> {
    let mut actions = Vec::new();

    ui.vertical(|ui| {
        ui.label(RichText::new(panel.title()).heading());
        if let Some(desc) = panel.description() {
            ui.label(RichText::new(desc).weak().small());
        }
        ui.add_space(4.0);

        match &panel.kind {
            PanelKind::Dashboard => actions.extend(dashboard(ui, panel, state)),
            PanelKind::Metric => metric(ui, panel),
            PanelKind::Gauge => gauge(ui, panel),
            PanelKind::Table => table(ui, panel),
            PanelKind::List => list(ui, panel),
            PanelKind::LineChart | PanelKind::AreaChart => line_chart(ui, panel),
            PanelKind::BarChart => bar_chart(ui, panel),
            PanelKind::Heatmap => heatmap(ui, panel),
            PanelKind::Markdown | PanelKind::Doc => {
                // Deliberately plain: pulling a Markdown renderer in would add
                // a dependency for one panel kind. Callers that want rich text
                // can special-case Doc before calling here.
                ui.label(panel.prop_str("body").unwrap_or_default());
            }
            k if k.is_form_kind() => actions.extend(form(ui, panel, state)),
            other => placeholder(ui, other),
        }
    });

    actions
}

// ── composite ───────────────────────────────────────────────────────────────

/// A dashboard lays its children out in columns, each rendered as a full panel.
///
/// Children flow into whichever column is currently shortest, so a tall table
/// in one column does not leave a hole under a short metric in the next — the
/// masonry rule. Order is preserved within a column, and the first row still
/// reads left to right.
fn dashboard(ui: &mut Ui, panel: &Panel, state: &mut FormState) -> Vec<PanelAction> {
    if panel.children.is_empty() {
        ui.label(
            RichText::new("no panels on this dashboard")
                .weak()
                .italics(),
        );
        return Vec::new();
    }

    // Two columns reads well at typical side-pane widths; a wider host can
    // split children by `slot()` itself before calling here.
    let columns = (panel.prop_f64("columns_per_row").unwrap_or(2.0).max(1.0) as usize)
        .min(panel.children.len());
    let mut actions = Vec::new();

    ui.columns(columns, |cols| {
        for child in &panel.children {
            // Shortest column so far takes the next child; ties go left.
            let target = (0..columns)
                .min_by(|a, b| {
                    cols[*a]
                        .min_rect()
                        .height()
                        .total_cmp(&cols[*b].min_rect().height())
                })
                .unwrap_or(0);
            let col = &mut cols[target];
            col.group(|ui| {
                ui.set_min_width(180.0);
                actions.extend(render_panel_with_state(ui, child, state));
            });
            col.add_space(12.0);
        }
    });

    actions
}

// ── display kinds ───────────────────────────────────────────────────────────

/// A single big number.
fn metric(ui: &mut Ui, panel: &Panel) {
    let value = first_cell(panel)
        .map(render_cell)
        .unwrap_or_else(|| "—".to_string());
    let unit = panel.prop_str("unit").unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(RichText::new(value).size(34.0).strong().monospace());
        if !unit.is_empty() {
            ui.label(RichText::new(unit).weak());
        }
    });
}

/// A bounded reading with a filled track. `props.min` / `props.max` bound it;
/// without them the gauge degrades to a metric rather than inventing a scale.
fn gauge(ui: &mut Ui, panel: &Panel) {
    let Some(value) = first_cell(panel).and_then(as_f64) else {
        return metric(ui, panel);
    };
    let min = panel.prop_f64("min").unwrap_or(0.0);
    let max = panel.prop_f64("max").unwrap_or(100.0);
    let frac = if (max - min).abs() < f64::EPSILON {
        0.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
    };

    ui.label(
        RichText::new(format!("{value:.3}"))
            .size(28.0)
            .strong()
            .monospace(),
    );

    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0_f32, ui.visuals().extreme_bg_color);
    let mut filled = rect;
    filled.set_width(rect.width() * frac);
    painter.rect_filled(filled, 3.0_f32, accent(ui));

    ui.label(RichText::new(format!("{min} – {max}")).weak().small());
}

fn table(ui: &mut Ui, panel: &Panel) {
    if panel.rows.is_empty() {
        return empty(ui);
    }
    let headers = panel.columns();

    egui::Grid::new(format!("dgp_table_{}_{}", panel.namespace, panel.id))
        .striped(true)
        .show(ui, |ui| {
            if !headers.is_empty() {
                for h in &headers {
                    ui.label(RichText::new(h).strong().small());
                }
                ui.end_row();
            }
            // Panels are dashboards, not data dumps; DataGrout caps live
            // source queries at 200 rows and this matches that ceiling.
            for row in panel.rows.iter().take(200) {
                for cell in row {
                    ui.label(RichText::new(render_cell(cell)).monospace().small());
                }
                ui.end_row();
            }
        });
}

fn list(ui: &mut Ui, panel: &Panel) {
    if panel.rows.is_empty() {
        return empty(ui);
    }
    for row in panel.rows.iter().take(200) {
        let text = row.iter().map(render_cell).collect::<Vec<_>>().join(" · ");
        ui.label(format!("• {text}"));
    }
}

/// A polyline over the last numeric column.
///
/// Suited to dashboard-scale series. A host with a high-rate live signal should
/// draw that directly rather than routing it through a panel: panel rows are
/// facts, and facts are the wrong granularity for a waveform.
fn line_chart(ui: &mut Ui, panel: &Panel) {
    let values = numeric_series(panel);
    if values.is_empty() {
        return empty(ui);
    }

    let height = panel.prop_f64("height").unwrap_or(120.0) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);

    let (min, max) = min_max(&values);
    let range = if (max - min).abs() < 1e-12 {
        1.0
    } else {
        max - min
    };
    let dx = if values.len() > 1 {
        rect.width() / (values.len() - 1) as f32
    } else {
        rect.width()
    };

    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let y = rect.bottom() - (((v - min) / range) as f32) * rect.height();
            egui::pos2(rect.left() + i as f32 * dx, y)
        })
        .collect();

    painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, accent(ui))));
    ui.label(
        RichText::new(format!("{} pts · {min:.3} … {max:.3}", values.len()))
            .weak()
            .small(),
    );
}

fn bar_chart(ui: &mut Ui, panel: &Panel) {
    let values = numeric_series(panel);
    if values.is_empty() {
        return empty(ui);
    }
    let height = panel.prop_f64("height").unwrap_or(120.0) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);

    let (min, max) = min_max(&values);
    let base = min.min(0.0);
    let range = if (max - base).abs() < 1e-12 {
        1.0
    } else {
        max - base
    };
    let bw = rect.width() / values.len() as f32;

    for (i, v) in values.iter().enumerate() {
        let h = (((v - base) / range) as f32) * rect.height();
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * bw, rect.bottom() - h),
            Vec2::new((bw - 2.0).max(1.0), h),
        );
        painter.rect_filled(bar, 1.0_f32, accent(ui));
    }
}

/// Rows as intensity bands.
fn heatmap(ui: &mut Ui, panel: &Panel) {
    if panel.rows.is_empty() {
        return empty(ui);
    }
    let cell = panel.prop_f64("cell").unwrap_or(8.0) as f32;
    let cols = panel.rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return empty(ui);
    }

    let size = Vec2::new(cols as f32 * cell, panel.rows.len() as f32 * cell);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);

    let all: Vec<f64> = panel.rows.iter().flatten().filter_map(as_f64).collect();
    let (min, max) = min_max(&all);
    let range = if (max - min).abs() < 1e-12 {
        1.0
    } else {
        max - min
    };

    for (r, row) in panel.rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            let Some(v) = as_f64(v) else { continue };
            let t = ((v - min) / range).clamp(0.0, 1.0) as f32;
            let px = egui::Rect::from_min_size(
                egui::pos2(rect.left() + c as f32 * cell, rect.top() + r as f32 * cell),
                Vec2::splat(cell),
            );
            painter.rect_filled(px, 0.0_f32, intensity(t));
        }
    }
}

// ── form kinds ──────────────────────────────────────────────────────────────

fn form(ui: &mut Ui, panel: &Panel, state: &mut FormState) -> Vec<PanelAction> {
    let mut actions = Vec::new();

    for field in &panel.fields {
        let label = field.label();
        ui.horizontal(|ui| {
            let text = if field.required() {
                format!("{label} *")
            } else {
                label.clone()
            };
            ui.label(RichText::new(text).small());
        });

        let id = field.id.clone();
        match field.kind {
            PanelKind::Checkbox => {
                let checked = state.checks.entry(id.clone()).or_default();
                if ui.checkbox(checked, "").changed() {
                    actions.push(PanelAction::ValueChanged {
                        panel_id: panel.id.clone(),
                        field_id: id,
                        value: checked.to_string(),
                    });
                }
            }
            PanelKind::Button => {
                if ui.button(&label).clicked() {
                    actions.push(PanelAction::Submit {
                        panel_id: panel.id.clone(),
                        field_id: id,
                    });
                }
            }
            PanelKind::TextArea | PanelKind::RichText => {
                let value = state
                    .values
                    .entry(id.clone())
                    .or_insert_with(|| field.default_value().unwrap_or_default());
                if ui.text_edit_multiline(value).changed() {
                    actions.push(PanelAction::ValueChanged {
                        panel_id: panel.id.clone(),
                        field_id: id,
                        value: value.clone(),
                    });
                }
            }
            _ => {
                let value = state
                    .values
                    .entry(id.clone())
                    .or_insert_with(|| field.default_value().unwrap_or_default());
                let widget = egui::TextEdit::singleline(value).hint_text(field.placeholder());
                if ui.add(widget).changed() {
                    actions.push(PanelAction::ValueChanged {
                        panel_id: panel.id.clone(),
                        field_id: id,
                        value: value.clone(),
                    });
                }
            }
        }
        ui.add_space(6.0);
    }

    actions
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn placeholder(ui: &mut Ui, kind: &PanelKind) {
    ui.label(
        RichText::new(format!("(no renderer for {})", kind.as_str()))
            .weak()
            .italics(),
    );
}

fn empty(ui: &mut Ui) {
    ui.label(RichText::new("no data").weak().italics());
}

fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// Dark-to-bright ramp for heatmap cells. Perceptually crude but theme-neutral
/// and dependency-free; swap for a real colormap if it starts carrying meaning.
fn intensity(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (20.0 + 200.0 * t) as u8,
        (30.0 + 120.0 * t) as u8,
        (60.0 + 60.0 * (1.0 - t)) as u8,
    )
}

fn first_cell(panel: &Panel) -> Option<&Value> {
    panel.rows.first()?.last()
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The last numeric cell of each row — chart rows are `[label, value]` by
/// convention, so the value is on the right.
fn numeric_series(panel: &Panel) -> Vec<f64> {
    panel
        .rows
        .iter()
        .filter_map(|row| row.iter().rev().find_map(as_f64))
        .collect()
}

fn min_max(values: &[f64]) -> (f64, f64) {
    values
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
}

fn render_cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datagrout_panels::{PanelFacts, Props};
    use serde_json::json;

    fn panel_with(kind: PanelKind, rows: Vec<Vec<Value>>) -> Panel {
        Panel {
            id: "t".into(),
            kind,
            namespace: "ns".into(),
            props: Props::new(),
            rows,
            source: None,
            fields: Vec::new(),
            children: Vec::new(),
            published: true,
        }
    }

    /// Run one headless frame and render `panel` inside it.
    ///
    /// egui needs no window to lay out and paint into shapes, so every kind can
    /// be exercised for real — panics, infinite loops, bad grid arithmetic —
    /// without a display. Returns the actions the frame produced.
    fn render_once(panel: &Panel) -> Vec<PanelAction> {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                actions = render_panel(ui, panel);
            });
        });
        actions
    }

    #[test]
    fn numeric_series_takes_the_rightmost_number() {
        let p = panel_with(
            PanelKind::LineChart,
            vec![
                vec![json!("Jan"), json!(4.0)],
                vec![json!("Feb"), json!(6.0)],
            ],
        );
        assert_eq!(numeric_series(&p), vec![4.0, 6.0]);
    }

    #[test]
    fn numeric_series_skips_rows_with_no_number() {
        let p = panel_with(
            PanelKind::LineChart,
            vec![
                vec![json!("Jan"), json!("n/a")],
                vec![json!("Feb"), json!(6.0)],
            ],
        );
        assert_eq!(numeric_series(&p), vec![6.0]);
    }

    #[test]
    fn min_max_of_a_flat_series_does_not_divide_by_zero() {
        let (lo, hi) = min_max(&[2.0, 2.0]);
        assert_eq!((lo, hi), (2.0, 2.0));
    }

    #[test]
    fn every_display_kind_renders_with_and_without_rows() {
        let kinds = [
            PanelKind::BarChart,
            PanelKind::LineChart,
            PanelKind::AreaChart,
            PanelKind::PieChart,
            PanelKind::Scatter,
            PanelKind::Table,
            PanelKind::Metric,
            PanelKind::Gauge,
            PanelKind::Markdown,
            PanelKind::List,
            PanelKind::Heatmap,
            PanelKind::Timeline,
            PanelKind::Funnel,
            PanelKind::Game,
            PanelKind::Doc,
            PanelKind::Dashboard,
            PanelKind::Unknown("sonar".into()),
        ];
        let rows = vec![
            vec![json!("a"), json!(1.5)],
            vec![json!("b"), json!(-2.0)],
            vec![json!("c"), json!("n/a")],
        ];
        for kind in kinds {
            render_once(&panel_with(kind.clone(), Vec::new()));
            render_once(&panel_with(kind, rows.clone()));
        }
    }

    #[test]
    fn a_flat_series_renders_without_dividing_by_zero() {
        let p = panel_with(PanelKind::BarChart, vec![vec![json!(3)], vec![json!(3)]]);
        render_once(&p);
        let g = panel_with(PanelKind::Gauge, vec![vec![json!(50)]]);
        render_once(&g);
    }

    #[test]
    fn a_dashboard_renders_its_children() {
        let facts = PanelFacts {
            panels: vec![
                json!({"Id": "board", "Kind": "dashboard", "Namespace": "ns"}),
                json!({"Id": "a", "Kind": "metric", "Namespace": "ns"}),
                json!({"Id": "b", "Kind": "table", "Namespace": "ns"}),
                json!({"Id": "c", "Kind": "bar_chart", "Namespace": "ns"}),
            ],
            props: vec![
                json!({"Id": "a", "Key": "parent", "Value": "board"}),
                json!({"Id": "b", "Key": "parent", "Value": "board"}),
                json!({"Id": "c", "Key": "parent", "Value": "board"}),
                json!({"Id": "b", "Key": "columns", "Value": ["Name", "N"]}),
            ],
            data: vec![
                json!({"Id": "a", "Rows": [["x", 7]]}),
                json!({"Id": "b", "Rows": [["p", 1], ["q", 2]]}),
            ],
            ..Default::default()
        };
        let board = Panel::all_from_facts(&facts).remove(0);
        assert_eq!(board.children.len(), 3);
        // Three children in a two-column grid exercises the row-wrap path.
        render_once(&board);
    }

    #[test]
    fn a_form_renders_every_field_kind_and_seeds_defaults() {
        let facts = PanelFacts {
            panels: vec![
                json!({"Id": "f", "Kind": "form", "Namespace": "ns"}),
                json!({"Id": "name", "Kind": "text_input", "Namespace": "ns"}),
                json!({"Id": "notes", "Kind": "textarea", "Namespace": "ns"}),
                json!({"Id": "ok", "Kind": "checkbox", "Namespace": "ns"}),
                json!({"Id": "go", "Kind": "button", "Namespace": "ns"}),
            ],
            props: vec![
                json!({"Id": "name", "Key": "parent", "Value": "f"}),
                json!({"Id": "name", "Key": "default", "Value": "Ada"}),
                json!({"Id": "notes", "Key": "parent", "Value": "f"}),
                json!({"Id": "ok", "Key": "parent", "Value": "f"}),
                json!({"Id": "go", "Key": "parent", "Value": "f"}),
            ],
            ..Default::default()
        };
        let form = Panel::all_from_facts(&facts).remove(0);
        assert_eq!(form.fields.len(), 4);

        let ctx = egui::Context::default();
        let mut state = FormState::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = render_panel_with_state(ui, &form, &mut state);
                // Nothing was clicked or typed in a headless frame.
                assert!(actions.is_empty());
            });
        });
        // A declared default seeds the field's state on first render.
        assert_eq!(state.values.get("name").map(String::as_str), Some("Ada"));
    }

    #[test]
    fn tables_use_the_columns_prop_for_headers() {
        let mut p = panel_with(PanelKind::Table, vec![vec![json!("INV-1"), json!(30)]]);
        p.props.insert("columns".into(), json!(["Invoice", "Days"]));
        assert_eq!(p.columns(), vec!["Invoice", "Days"]);
        render_once(&p);
    }
}
