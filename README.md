# Smart Panels

Render [DataGrout](https://datagrout.ai) **Smart Panels** — declarative UI stored
as logic-cell facts — on any surface.

```bash
cargo add datagrout-panels            # the model
cargo add datagrout-panels-egui       # native GUI renderer
cargo add datagrout-panels-mcp        # MCP Apps transpiler
```

```prolog
panel(pipeline_dashboard, dashboard, pipeline_pulse).
panel_prop(pipeline_dashboard, title, 'Pipeline Pulse').

panel(stage_mix, bar_chart, pipeline_pulse).
panel_prop(stage_mix, parent, pipeline_dashboard).
panel_source(stage_mix, pipeline_pulse, 'stage_count(Stage, N)').
```

A panel is not a JSON blob and not a template. It is knowledge in a logic cell:
queryable, composable, versioned, and **derived** — its `panel_source` goal
re-runs against the rulebase every time the panel is read.

## One model, many surfaces

The model crate parses facts into a renderer-agnostic tree and stops. Renderers
are separate, so a consumer that only transpiles never links a GUI toolkit, and
adding a backend never touches the model.

| crate | surface | status |
|---|---|---|
| `datagrout-panels` | the model — facts → `Panel` tree | ✅ |
| `datagrout-panels-egui` | native immediate-mode GUI | ✅ |
| `datagrout-panels-mcp` | MCP Apps (SEP-1865) `ui://` resources | ✅ |
| `datagrout-panels-tui` | terminal, via ratatui | planned |

### Why immediate mode is the natural fit

egui redraws from state every frame with no retained widget tree. A Smart Panel
re-derives its rows from the rulebase on every read with no retained DOM. They
are the same architecture — so an immediate-mode renderer pays no reconciliation
cost that a retained-tree renderer must.

### Why MCP Apps transpiles cleanly

[MCP Apps][sep] — the first official MCP extension, released 2026-01-26 — lets a
server hand a host an interactive UI as a `ui://` resource with mime type
`text/html;profile=mcp-app`, rendered in a sandboxed iframe that speaks
JSON-RPC over `postMessage`. Tools link to their view via `_meta.ui.resourceUri`.

Crucially, an MCP App receives its data by notification
(`ui/notifications/tool-result`) rather than fetching it up front — which is
exactly how a Smart Panel already behaves. The panel's kind and props become the
document; the rows arrive at render time. So one definition, stored once as
facts, drives a native GUI *and* an MCP host with no second authoring step and
no drift between them.

## Usage

The intended input is the gateway's own `smart_panel.list` response — one call,
props included, dashboards resolved with their children:

```rust
use datagrout_panels::Panel;
use datagrout_panels_egui::render_panel;

// `list_response` is what `smart_panel.list` returned, however you called it.
let panels = Panel::all_from_list(&list_response);

egui::CentralPanel::default().show(ctx, |ui| {
    for panel in &panels {
        render_panel(ui, panel);
    }
});
```

Raw `logic.query` rows are accepted too when you need full row sets or field
metadata — see `Panel::all_from_facts` and `datagrout_panels::goals`.

Transpiling the same panel for an MCP host:

```rust
use datagrout_panels_mcp::{to_ui_resource, tool_meta, TranspileOptions};

let resource = to_ui_resource(&panel, &TranspileOptions {
    server: "my-app".into(),
    ..Default::default()
});

// Publish `resource.to_resource_json()` as a resource, and tag the tool that
// produces its rows:
let meta = tool_meta(&resource.uri, true);
```

## Checked against a live cell

The model was corrected against panels published to a real DataGrout account,
not written from the schema alone. What that changed is recorded in
[`SPEC.md`](SPEC.md) and the [`CHANGELOG`](CHANGELOG.md) — in short: parts are
linked by the **child's `parent` prop**, `dashboard` is a kind, props are
**typed** (`columns` is a list), and identity is `(namespace, id)`.

`examples/parse_fixture.rs` runs a captured fact dump through the parser and
reports what came out; it is how the crate stays honest as the schema moves.

## Language parity

Rust is the reference implementation. The **model and fact parsing** are the
portable core and should exist in every supported language; renderers are
whatever suits that language. [`SPEC.md`](SPEC.md) is the contract a port
implements.

| language | model | renderers |
|---|---|---|
| Rust | ✅ | egui, MCP Apps |
| TypeScript | planned | MCP Apps, DOM |

## Security

Panel facts may have been asserted by an agent. Treat every prop as untrusted:
the MCP transpiler escapes all panel-authored text, declares no CSP domains by
default, and emits no callback path for display-only panels.

## Layout

```
datagrout-panels/
├── SPEC.md          the fact schema + renderer contract (portable)
├── CHANGELOG.md
├── rust/            reference implementation (three crates)
└── typescript/      (planned)
```

## License

`MIT OR Apache-2.0`, at your option.

[sep]: https://modelcontextprotocol.io/seps/1865-mcp-apps-interactive-user-interfaces-for-mcp
