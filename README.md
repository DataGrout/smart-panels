# Smart Panels

[![CI](https://github.com/DataGrout/smart-panels/actions/workflows/ci.yml/badge.svg)](https://github.com/DataGrout/smart-panels/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/datagrout-panels.svg?label=datagrout-panels)](https://crates.io/crates/datagrout-panels)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Render [DataGrout](https://datagrout.ai) **Smart Panels** — declarative UI stored
as logic-cell facts — on any surface.

```bash
cargo add datagrout-panels            # the model
cargo add datagrout-panels-egui       # native GUI renderer
cargo add datagrout-panels-mcp        # MCP Apps transpiler
```

## A panel is facts, not a blob

You create a panel by calling the gateway's `smart_panel.publish` tool:

```json
{
  "id": "stage_mix",
  "kind": "bar_chart",
  "namespace": "pipeline_pulse",
  "props": { "title": "Open Opportunities by Stage" },
  "source": { "namespace": "pipeline_pulse", "query": "stage_count(Stage, N)" }
}
```

DataGrout compiles that into Prolog facts in the `_panels` namespace of a logic
cell — the same knowledge base the rest of your rules and data live in:

```prolog
panel(stage_mix, bar_chart, pipeline_pulse).
panel_prop(stage_mix, title, 'Open Opportunities by Stage').
panel_source(stage_mix, pipeline_pulse, 'stage_count(Stage, N)').
```

Read positionally:

| fact | arguments |
|---|---|
| `panel/3` | panel id, kind, and the namespace that owns it |
| `panel_prop/3` | panel id, a config key, its value — one fact per prop |
| `panel_source/3` | panel id, the namespace to query, and a **Prolog goal** |
| `panel_data/2` | panel id and a static row snapshot, for a panel with no query |

`stage_count(Stage, N)` is an ordinary goal against that cell, so whatever it
proves is what the chart draws: one row per solution, `Stage` and `N` as the
columns. Nothing is copied into the panel, and the goal re-runs on every read.
That is what makes a panel **derived** rather than stored — and queryable,
composable and versioned like any other knowledge in the cell.

You would not normally write these facts by hand; `smart_panel.publish` does
it. But they are facts like any others, so `logic.query` can read them and
rules can reason over them.

### Composition is a fact too

A container does not list its parts. Each part names its container in a
`parent` prop, so a dashboard is one panel plus children pointing at it:

```prolog
panel(pipeline_dashboard, dashboard, pipeline_pulse).
panel_prop(pipeline_dashboard, title, 'Pipeline Pulse').
panel_prop(stage_mix, parent, pipeline_dashboard).
```

Published as `kind: "dashboard"` for the container and
`"props": { "parent": "pipeline_dashboard" }` on each child. A form is
`kind: "form"` with a `fields` array, where each field is itself a mini-panel
that may declare `inputs` (sibling fields it depends on), a `trigger` and an
`emit`.

The parser inverts those edges: a resolved `Panel` carries its `children` or
`fields`, and the parts do not appear at the top level.

### Kinds and props are a fixed vocabulary

Display kinds are `bar_chart`, `line_chart`, `pie_chart`, `scatter`, `table`,
`metric`, `gauge`, `markdown`, `list`, `area_chart`, `heatmap`, `timeline`,
`funnel`, `game`, `doc` and `dashboard`. Form kinds are `form`, `text_input`,
`textarea`, `dropdown`, `select`, `checkbox`, `radio`, `button`,
`number_input`, `date_input`, `file_upload` and `rich_text`. Props seen most
often are `title`, `description`, `parent`, `slot`, `columns` (a **list** of
table headers) and `published`.

[`SPEC.md`](SPEC.md) is the full contract: every kind, every observed prop with
its type, how rows normalize, and what a renderer must do with a kind it does
not recognise — draw a labelled placeholder, never fail.

## What makes them smart

Storing UI as facts would be a curiosity on its own. Four properties are what
the name is actually pointing at.

**The rows are inferred, not fetched.** `panel_source` holds a goal, so it can
call *rules*, not just match stored facts:

```prolog
at_risk(Deal) :-
    stage(Deal, Stage), late_stage(Stage),
    days_since_contact(Deal, Days), Days > 14.
```

A panel over `at_risk(Deal)` shows whatever satisfies that rule right now. The
definition of "at risk" lives in the cell, so changing it changes every panel
built on it, and no panel had to be edited.

**It is current by construction.** The goal re-runs on every read, so a fact an
agent asserted a second ago is already in the panel. There is no cache to
invalidate, no build step, and no refresh path to get wrong.

**An agent can build the UI.** Because a panel is just facts, anything holding
`smart_panel.publish` can create or amend one as part of doing its work — which
is why props carry `created_by_agent` provenance. A dashboard can be an outcome
of reasoning rather than something a person laid out in advance.

**A form is a small dataflow graph.** Fields declare what they depend on
(`field_input`), when they fire (`field_trigger`), and what happens to their
output (`field_emit`) — and a field's `panel_source` can invoke a tool, skill
or workflow rather than query data. So a field can take another field's value,
call something with it, and replace its own contents with the result:

```json
{
  "id": "enriched_desc",
  "kind": "textarea",
  "props": { "label": "Description" },
  "inputs": ["company_name"],
  "trigger": "on_event",
  "emit": "replacement",
  "source": { "query": "enrich_company(CompanyName, Out)" }
}
```

Driving that graph is not client-side work. On DataGrout, submitting a form runs
the panel's goal **in the cell**: if the goal's functor is a rule published with
`reactor.expose`, the fields bind to that rule's declared `+` inputs and its `-`
outputs come back shaped by the contract; otherwise the fields bind into the
`panel_source` goal itself, matching field ids to the goal's Prolog variables
and collecting the unbound ones as outputs. Either way it runs under the cell's
sandbox with a bounded timeout, and a rule body may call tools — so a field can
invoke a skill or workflow with no application code in between.

What a renderer does is collect values and hand the interaction back. These
crates return `PanelAction`s rather than dispatching them because *where* to
submit is the host's choice, even though *what runs* is already defined in the
cell.

**The same rule can also be an HTTP endpoint.** `reactor.expose` publishes an
LC rule with a mode contract — `+invoice:atom, -result:list`, where `+` binds
from the request and `-` is returned — and DataGrout then serves it as JSON
*and* as the backing for a panel form, from one definition inside one sandbox.
Facts supplied with a request are asserted ephemerally (assert under a unique
tag, query, retract), so submitted data is not retained between calls.

And since the definitions are facts, they are queryable and auditable like
anything else in the cell — `logic.query` can ask which panels exist, which are
published, or which an agent created.

## Where panels live, and how you read them

A logic cell is scoped to one account **and one hub server**, so panels
published through one server are not visible through another: which panels you
see depends on which server you connected to. Within `_panels`, each panel's
own `namespace` — `pipeline_pulse` above — groups it with its siblings.

Anything that speaks MCP can publish: an agent handed the tool, your own code
through an MCP client such as
[conduit-sdk](https://github.com/DataGrout/conduit-sdk), or the Smart Panels
page in the DataGrout web app. Reading them back is a single
`smart_panel.list` call, and that response is exactly what
`Panel::all_from_list` parses.

**These crates render panels; they neither create nor fetch them, and have no
transport.** You bring the MCP client; they turn what it returned into
something on screen.

### Trying it without an account

The parser takes plain JSON, so a hand-written list response renders like a
real one — useful for tests, examples, and seeing the renderers work before you
have panels of your own:

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
```

## One model, many surfaces

The model crate parses facts into a renderer-agnostic tree and stops. Renderers
are separate, so a consumer that only transpiles never links a GUI toolkit, and
adding a backend never touches the model.

| crate | surface | version | docs |
|---|---|---|---|
| [`datagrout-panels`](rust/datagrout-panels) | the model — facts → `Panel` tree | [![crates.io](https://img.shields.io/crates/v/datagrout-panels.svg)](https://crates.io/crates/datagrout-panels) | [![docs.rs](https://img.shields.io/docsrs/datagrout-panels)](https://docs.rs/datagrout-panels) |
| [`datagrout-panels-egui`](rust/datagrout-panels-egui) | native immediate-mode GUI | [![crates.io](https://img.shields.io/crates/v/datagrout-panels-egui.svg)](https://crates.io/crates/datagrout-panels-egui) | [![docs.rs](https://img.shields.io/docsrs/datagrout-panels-egui)](https://docs.rs/datagrout-panels-egui) |
| [`datagrout-panels-mcp`](rust/datagrout-panels-mcp) | MCP Apps (SEP-1865) `ui://` resources | [![crates.io](https://img.shields.io/crates/v/datagrout-panels-mcp.svg)](https://crates.io/crates/datagrout-panels-mcp) | [![docs.rs](https://img.shields.io/docsrs/datagrout-panels-mcp)](https://docs.rs/datagrout-panels-mcp) |
| `datagrout-panels-tui` | terminal, via ratatui | planned | |

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
├── publish.sh       releases whichever crates crates.io lacks at the workspace version
├── rust/            reference implementation (three crates)
└── typescript/      (planned)
```

## Releasing

The three crates share one version, set in `rust/Cargo.toml` under
`[workspace.package]`, and the renderer and transpiler depend on the model at
exactly that version. A release commit bumps it and adds a dated entry to
`CHANGELOG.md`; `./publish.sh` then publishes every crate crates.io does not
yet have at that version, model first, and tags `v<version>`. It is safe to
re-run: crates already up are skipped. `./publish.sh --dry-run` packages and
verifies without uploading.

## License

`MIT OR Apache-2.0`, at your option.

[sep]: https://modelcontextprotocol.io/seps/1865-mcp-apps-interactive-user-interfaces-for-mcp
