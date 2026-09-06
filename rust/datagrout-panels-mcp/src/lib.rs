//! Transpile Smart Panels into **MCP Apps** ([SEP-1865]) resources.
//!
//! MCP Apps — the first official MCP extension, released 2026-01-26 — lets a
//! server hand a host an interactive UI: an HTML document published as a
//! `ui://` resource with mime type `text/html;profile=mcp-app`, rendered in a
//! sandboxed iframe that talks JSON-RPC to the host over `postMessage`. A tool
//! links to its view through `_meta.ui.resourceUri`.
//!
//! # Why the two models line up
//!
//! A Smart Panel is a *declarative* description of a view: kind, props, and a
//! query that re-derives its rows. An MCP App is a *document* that receives its
//! data by notification (`ui/notifications/tool-result`) rather than fetching
//! it up front. So a panel transpiles cleanly: the panel's kind and props
//! become a static template, and the rows arrive at render time exactly as the
//! extension already intends.
//!
//! That means one panel definition, stored once as facts, can drive a native
//! GUI (`datagrout-panels-egui`) and an MCP host — with no second authoring
//! step and no divergence between the two.
//!
//! # Delivering rows
//!
//! The host sends `ui/notifications/tool-result`. The document reads rows from
//! `structuredContent`:
//!
//! * a single panel: `structuredContent.rows` (or the whole `structuredContent`
//!   if it is an array);
//! * a dashboard: `structuredContent.panels`, an object keyed by child panel
//!   id, each value a rows array.
//!
//! # What this crate does and does not do
//!
//! It emits documents and metadata. It does **not** serve them, register them,
//! or speak MCP — a server does that with whatever MCP library it already uses.
//! Keeping it a pure function of a [`Panel`] means it is testable without a
//! host and embeddable in any server.
//!
//! [SEP-1865]: https://modelcontextprotocol.io/seps/1865-mcp-apps-interactive-user-interfaces-for-mcp

#![forbid(unsafe_code)]

use datagrout_panels::{Panel, PanelKind};
use serde_json::{json, Value};

/// The only mime type the MCP Apps MVP supports.
pub const MCP_APP_MIME: &str = "text/html;profile=mcp-app";

/// The capability key a host announces UI support under.
pub const UI_CAPABILITY: &str = "io.modelcontextprotocol/ui";

/// A transpiled panel, ready to publish as an MCP resource.
#[derive(Debug, Clone)]
pub struct UiResource {
    /// `ui://<server>/<panel-id>`
    pub uri: String,
    pub mime_type: &'static str,
    /// The full HTML document.
    pub text: String,
    /// Contents of the resource's `_meta.ui` object.
    pub meta: Value,
}

impl UiResource {
    /// The resource as an MCP `resources/read` payload.
    pub fn to_resource_json(&self) -> Value {
        json!({
            "uri": self.uri,
            "mimeType": self.mime_type,
            "text": self.text,
            "_meta": { "ui": self.meta },
        })
    }
}

/// Transpiler options.
pub struct TranspileOptions {
    /// Server segment of the `ui://` authority, e.g. `"my-app"`.
    pub server: String,
    /// Whether the view may call back into the host.
    ///
    /// A read-only display panel needs no callbacks; a form does. Defaulting to
    /// `false` keeps the emitted view as inert as its content allows.
    pub interactive: bool,
    /// Host-visible border hint (`_meta.ui.prefersBorder`).
    pub prefers_border: bool,
}

impl Default for TranspileOptions {
    fn default() -> Self {
        Self {
            server: "datagrout".to_string(),
            interactive: false,
            prefers_border: true,
        }
    }
}

/// Transpile a panel into a `ui://` resource.
pub fn to_ui_resource(panel: &Panel, opts: &TranspileOptions) -> UiResource {
    let uri = ui_uri(&opts.server, &panel.id);
    let interactive = opts.interactive || panel.kind.is_form_kind();

    // No CSP domains are declared: the emitted document inlines its styles and
    // script and loads nothing. Widening this is the caller's decision to make
    // explicitly, never a default.
    let mut meta = json!({ "prefersBorder": opts.prefers_border });
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("csp".into(), json!({}));
    }

    UiResource {
        uri,
        mime_type: MCP_APP_MIME,
        text: render_document(panel, interactive),
        meta,
    }
}

/// The `_meta` a tool carries to link itself to a panel's view.
///
/// Uses the nested `_meta.ui.*` form; the flat `_meta["ui/resourceUri"]` key is
/// deprecated and deliberately not emitted.
pub fn tool_meta(panel_uri: &str, visible_to_model: bool) -> Value {
    let visibility = if visible_to_model {
        json!(["model", "app"])
    } else {
        json!(["app"])
    };
    json!({ "ui": { "resourceUri": panel_uri, "visibility": visibility } })
}

/// Build the `ui://` URI for a panel.
///
/// Ids are lowercased and non-alphanumerics collapse to `-` so a Prolog atom
/// like `revenue_chart` yields a well-formed authority path.
pub fn ui_uri(server: &str, panel_id: &str) -> String {
    format!("ui://{}/{}", slug(server), slug(panel_id))
}

fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true; // suppress a leading dash
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ── document emission ───────────────────────────────────────────────────────

/// Emit the panel's HTML document.
///
/// The document carries the panel's *shape* — kind, title, field structure —
/// and a small runtime that fills in rows when the host sends
/// `ui/notifications/tool-result`. Baking rows in at transpile time would
/// freeze data that the panel model defines as re-derived on every read.
fn render_document(panel: &Panel, interactive: bool) -> String {
    let title = escape(&panel.title());
    let kind = slug(panel.kind.as_str());
    let description = panel
        .description()
        .map(|d| format!("<p class=\"desc\">{}</p>", escape(&d)))
        .unwrap_or_default();
    let body = body_for(panel);
    let script = runtime_script(interactive);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ margin: 0; padding: 12px; font: 13px/1.45 system-ui, -apple-system, sans-serif; }}
  h1 {{ font-size: 15px; margin: 0 0 2px; }}
  h2 {{ font-size: 13px; margin: 0 0 4px; }}
  .desc {{ margin: 0 0 10px; opacity: .7; font-size: 12px; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ text-align: left; padding: 4px 8px; border-bottom: 1px solid rgba(128,128,128,.25); }}
  th {{ font-weight: 600; font-size: 11px; text-transform: uppercase; opacity: .7; }}
  .metric {{ font-size: 30px; font-weight: 600; font-variant-numeric: tabular-nums; }}
  .track {{ height: 10px; border-radius: 3px; background: rgba(128,128,128,.2); overflow: hidden; }}
  .fill {{ height: 100%; width: 0; background: currentColor; }}
  .empty {{ opacity: .55; font-style: italic; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px; }}
  .card {{ border: 1px solid rgba(128,128,128,.25); border-radius: 6px; padding: 10px; }}
  .card ul {{ margin: 0; padding-left: 16px; }}
  label {{ display: block; font-size: 11px; opacity: .75; margin: 8px 0 2px; }}
  input, textarea, select {{ width: 100%; box-sizing: border-box; padding: 5px 7px;
    border: 1px solid rgba(128,128,128,.4); border-radius: 5px; background: transparent;
    color: inherit; font: inherit; }}
</style>
</head>
<body data-panel-kind="{kind}" data-panel-id="{id}">
<h1>{title}</h1>
{description}
{body}
{script}
</body>
</html>"#,
        id = escape(&panel.id),
    )
}

fn body_for(panel: &Panel) -> String {
    match &panel.kind {
        PanelKind::Dashboard => {
            let cards: String = panel
                .children
                .iter()
                .map(|child| {
                    let id = escape(&child.id);
                    let title = escape(&child.title());
                    let kind = slug(child.kind.as_str());
                    // Every child gets the same container: a value slot for
                    // metric-like kinds and a list slot for everything else.
                    // The runtime picks by `data-kind`.
                    format!(
                        r#"<section class="card" data-child="{id}" data-kind="{kind}"><h2>{title}</h2><div class="metric" data-value hidden>—</div><ul data-rows></ul><p class="empty" data-empty>no data</p></section>"#
                    )
                })
                .collect();
            if cards.is_empty() {
                r#"<p class="empty">no panels on this dashboard</p>"#.to_string()
            } else {
                format!(r#"<div class="grid" id="dg-dashboard">{cards}</div>"#)
            }
        }
        PanelKind::Metric | PanelKind::Gauge => {
            let track = if matches!(panel.kind, PanelKind::Gauge) {
                r#"<div class="track"><div class="fill" id="dg-fill"></div></div>"#
            } else {
                ""
            };
            format!(r#"<div class="metric" id="dg-value">—</div>{track}"#)
        }
        PanelKind::Table => {
            let headers: String = panel
                .columns()
                .iter()
                .map(|c| format!("<th>{}</th>", escape(c)))
                .collect();
            format!(
                r#"<table><thead><tr id="dg-head">{headers}</tr></thead><tbody id="dg-rows"></tbody></table>
<p class="empty" id="dg-empty">no data</p>"#
            )
        }
        k if k.is_form_kind() => {
            let fields: String = panel
                .fields
                .iter()
                .map(|f| {
                    let label = escape(&f.label());
                    let id = escape(&f.id);
                    let required = if f.required() { " *" } else { "" };
                    let control = match f.kind {
                        PanelKind::TextArea | PanelKind::RichText => {
                            format!(r#"<textarea id="{id}" rows="3"></textarea>"#)
                        }
                        PanelKind::Checkbox => {
                            format!(r#"<input id="{id}" type="checkbox">"#)
                        }
                        PanelKind::NumberInput => {
                            format!(r#"<input id="{id}" type="number">"#)
                        }
                        PanelKind::DateInput => format!(r#"<input id="{id}" type="date">"#),
                        PanelKind::Button => {
                            return format!(r#"<button id="{id}" data-dg-submit>{label}</button>"#)
                        }
                        _ => format!(
                            r#"<input id="{id}" type="text" placeholder="{}">"#,
                            escape(&f.placeholder())
                        ),
                    };
                    format!("<label for=\"{id}\">{label}{required}</label>{control}")
                })
                .collect();
            format!(r#"<form id="dg-form">{fields}</form>"#)
        }
        // Charts render as a labelled series list rather than a fabricated
        // canvas: an MCP host draws in a small iframe of unknown size, and a
        // readable table beats an unreadable plot.
        _ => r#"<ul id="dg-series"></ul><p class="empty" id="dg-empty">no data</p>"#.to_string(),
    }
}

/// The in-document runtime: the SEP-1865 handshake plus row rendering.
fn runtime_script(interactive: bool) -> String {
    let submit = if interactive {
        r#"
  document.querySelectorAll('[data-dg-submit]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var values = {};
      document.querSelectorAll('#dg-form [id]').forEach(function (el) {
        values[el.id] = el.type === 'checkbox' ? el.checked : el.value;
      });
      send('ui/message', { text: JSON.stringify(values) });
    });
  });"#
            .replace("querSelectorAll", "querySelectorAll")
    } else {
        String::new()
    };

    format!(
        r#"<script>
(function () {{
  var seq = 0;
  function send(method, params) {{
    parent.postMessage({{ jsonrpc: '2.0', id: ++seq, method: method, params: params || {{}} }}, '*');
  }}
  function notify(method, params) {{
    parent.postMessage({{ jsonrpc: '2.0', method: method, params: params || {{}} }}, '*');
  }}

  function lastValue(rows) {{
    var last = rows.length ? rows[rows.length - 1] : null;
    return Array.isArray(last) ? last[last.length - 1] : last;
  }}

  function fillList(list, rows) {{
    list.textContent = '';
    rows.slice(0, 200).forEach(function (row) {{
      var li = document.createElement('li');
      li.textContent = (Array.isArray(row) ? row : [row]).join(' · ');
      list.appendChild(li);
    }});
  }}

  function renderChild(card, rows) {{
    var kind = card.dataset.kind;
    var empty = card.querySelector('[data-empty]');
    if (empty) empty.hidden = rows.length > 0;
    var value = card.querySelector('[data-value]');
    var list = card.querySelector('[data-rows]');
    if (kind === 'metric' || kind === 'gauge') {{
      if (value) {{ value.hidden = false; var v = lastValue(rows); value.textContent = (v === null || v === undefined) ? '—' : v; }}
      if (list) list.hidden = true;
      return;
    }}
    if (list) fillList(list, rows);
  }}

  function render(structured) {{
    var kind = document.body.dataset.panelKind;

    if (kind === 'dashboard') {{
      var byId = (structured && structured.panels) || {{}};
      document.querySelectorAll('[data-child]').forEach(function (card) {{
        var rows = byId[card.dataset.child];
        renderChild(card, Array.isArray(rows) ? rows : []);
      }});
      return;
    }}

    var rows = [];
    if (structured && Array.isArray(structured.rows)) rows = structured.rows;
    else if (Array.isArray(structured)) rows = structured;

    var empty = document.getElementById('dg-empty');
    if (empty) empty.style.display = rows.length ? 'none' : '';

    if (kind === 'metric' || kind === 'gauge') {{
      var v = lastValue(rows);
      var el = document.getElementById('dg-value');
      if (el) el.textContent = (v === null || v === undefined) ? '—' : v;
      var fill = document.getElementById('dg-fill');
      if (fill && typeof v === 'number') {{
        var lo = Number(document.body.dataset.min || 0);
        var hi = Number(document.body.dataset.max || 100);
        var f = hi === lo ? 0 : Math.max(0, Math.min(1, (v - lo) / (hi - lo)));
        fill.style.width = (f * 100) + '%';
      }}
      return;
    }}

    var tbody = document.getElementById('dg-rows');
    if (tbody) {{
      tbody.textContent = '';
      rows.slice(0, 200).forEach(function (row) {{
        var tr = document.createElement('tr');
        (Array.isArray(row) ? row : [row]).forEach(function (cell) {{
          var td = document.createElement('td');
          td.textContent = cell === null || cell === undefined ? '' : String(cell);
          tr.appendChild(td);
        }});
        tbody.appendChild(tr);
      }});
      return;
    }}

    var series = document.getElementById('dg-series');
    if (series) fillList(series, rows);
  }}

  window.addEventListener('message', function (event) {{
    var msg = event.data;
    if (!msg || typeof msg !== 'object') return;
    if (msg.method === 'ui/notifications/tool-result') {{
      render(msg.params && msg.params.structuredContent);
    }}
  }});
{submit}

  send('ui/initialize', {{}});
  notify('ui/notifications/initialized', {{}});
}})();
</script>"#
    )
}

/// Escape text for HTML text and attribute contexts.
///
/// Panel props are authored by whoever wrote the facts, and those facts may
/// have been asserted by an agent. Treat every one of them as untrusted.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use datagrout_panels::PanelFacts;
    use serde_json::json;

    fn panel_from(facts: PanelFacts) -> Panel {
        Panel::all_from_facts(&facts).into_iter().next().unwrap()
    }

    fn metric_panel() -> Panel {
        panel_from(PanelFacts {
            panels: vec![json!({"Id": "rms_now", "Kind": "metric", "Namespace": "app"})],
            props: vec![json!({"Id": "rms_now", "Key": "title", "Value": "RMS"})],
            ..Default::default()
        })
    }

    #[test]
    fn uri_is_well_formed_from_a_prolog_atom() {
        assert_eq!(
            ui_uri("my-app", "revenue_chart"),
            "ui://my-app/revenue-chart"
        );
    }

    #[test]
    fn slug_collapses_runs_and_trims_edges() {
        assert_eq!(slug("__a  b__"), "a-b");
    }

    #[test]
    fn resource_carries_the_only_supported_mime_type() {
        let res = to_ui_resource(&metric_panel(), &TranspileOptions::default());
        assert_eq!(res.mime_type, "text/html;profile=mcp-app");
        assert!(res.text.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn document_declares_the_handshake() {
        let res = to_ui_resource(&metric_panel(), &TranspileOptions::default());
        assert!(res.text.contains("ui/initialize"));
        assert!(res.text.contains("ui/notifications/initialized"));
        assert!(res.text.contains("ui/notifications/tool-result"));
    }

    #[test]
    fn rows_are_not_baked_in_at_transpile_time() {
        let mut facts = PanelFacts {
            panels: vec![json!({"Id": "t", "Kind": "table", "Namespace": "ns"})],
            ..Default::default()
        };
        facts.data = vec![json!({"Id": "t", "Rows": [["secret_value", 1]]})];

        let res = to_ui_resource(&panel_from(facts), &TranspileOptions::default());
        // A panel is re-derived on every read; freezing a snapshot into the
        // document would contradict the model and leak stale data.
        assert!(!res.text.contains("secret_value"));
    }

    #[test]
    fn table_headers_come_from_the_columns_list() {
        let facts = PanelFacts {
            panels: vec![json!({"Id": "t", "Kind": "table", "Namespace": "ns"})],
            props: vec![json!({"Id": "t", "Key": "columns", "Value": ["Invoice", "Days & Co"]})],
            ..Default::default()
        };
        let res = to_ui_resource(&panel_from(facts), &TranspileOptions::default());
        assert!(res.text.contains("<th>Invoice</th>"));
        assert!(res.text.contains("<th>Days &amp; Co</th>"));
    }

    #[test]
    fn a_dashboard_emits_one_card_per_child_and_reads_rows_by_id() {
        let facts = PanelFacts {
            panels: vec![
                json!({"Id": "board", "Kind": "dashboard", "Namespace": "ns"}),
                json!({"Id": "total", "Kind": "metric", "Namespace": "ns"}),
                json!({"Id": "mix", "Kind": "bar_chart", "Namespace": "ns"}),
            ],
            props: vec![
                json!({"Id": "board", "Key": "title", "Value": "Pulse"}),
                json!({"Id": "total", "Key": "parent", "Value": "board"}),
                json!({"Id": "total", "Key": "title", "Value": "Total"}),
                json!({"Id": "mix", "Key": "parent", "Value": "board"}),
            ],
            ..Default::default()
        };
        let res = to_ui_resource(&panel_from(facts), &TranspileOptions::default());
        assert!(res.text.contains(r#"data-child="total""#));
        assert!(res.text.contains(r#"data-child="mix""#));
        assert!(res.text.contains(r#"data-kind="metric""#));
        assert!(res.text.contains("<h2>Total</h2>"));
        // The runtime resolves each child's rows from structuredContent.panels.
        assert!(res.text.contains("structured.panels"));
    }

    #[test]
    fn tool_meta_uses_the_nested_key_not_the_deprecated_flat_one() {
        let meta = tool_meta("ui://my-app/rms-now", true);
        assert_eq!(meta["ui"]["resourceUri"], json!("ui://my-app/rms-now"));
        assert_eq!(meta["ui"]["visibility"], json!(["model", "app"]));
        assert!(meta.get("ui/resourceUri").is_none());
    }

    #[test]
    fn app_only_visibility_omits_the_model() {
        let meta = tool_meta("ui://my-app/x", false);
        assert_eq!(meta["ui"]["visibility"], json!(["app"]));
    }

    #[test]
    fn form_panels_are_interactive_even_when_not_requested() {
        let facts = PanelFacts {
            panels: vec![
                json!({"Id": "f", "Kind": "form", "Namespace": "ns"}),
                json!({"Id": "go", "Kind": "button", "Namespace": "ns"}),
            ],
            props: vec![
                json!({"Id": "go", "Key": "parent", "Value": "f"}),
                json!({"Id": "go", "Key": "label", "Value": "Run"}),
            ],
            ..Default::default()
        };
        let res = to_ui_resource(&panel_from(facts), &TranspileOptions::default());
        assert!(res.text.contains("data-dg-submit"));
        assert!(res.text.contains("ui/message"));
        assert!(res.text.contains("querySelectorAll('#dg-form [id]')"));
    }

    #[test]
    fn display_panels_emit_no_callback_path() {
        let res = to_ui_resource(&metric_panel(), &TranspileOptions::default());
        assert!(!res.text.contains("ui/message"));
    }

    #[test]
    fn panel_text_is_escaped() {
        let facts = PanelFacts {
            panels: vec![json!({"Id": "x", "Kind": "metric", "Namespace": "ns"})],
            props: vec![
                json!({"Id": "x", "Key": "title", "Value": "<img src=x onerror=alert(1)>"}),
            ],
            ..Default::default()
        };
        let res = to_ui_resource(&panel_from(facts), &TranspileOptions::default());
        assert!(!res.text.contains("<img src=x"));
        assert!(res.text.contains("&lt;img"));
    }

    #[test]
    fn resource_json_nests_meta_under_ui() {
        let res = to_ui_resource(&metric_panel(), &TranspileOptions::default());
        let json = res.to_resource_json();
        assert_eq!(json["mimeType"], json!(MCP_APP_MIME));
        assert!(json["_meta"]["ui"]["prefersBorder"].is_boolean());
    }
}
