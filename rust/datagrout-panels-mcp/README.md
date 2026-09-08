# datagrout-panels-mcp

[![crates.io](https://img.shields.io/crates/v/datagrout-panels-mcp.svg)](https://crates.io/crates/datagrout-panels-mcp)
[![docs.rs](https://img.shields.io/docsrs/datagrout-panels-mcp)](https://docs.rs/datagrout-panels-mcp)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Transpile [DataGrout](https://datagrout.ai) Smart Panels into
**MCP Apps** ([SEP-1865]) `ui://` resources.

MCP Apps — the first official MCP extension, released 2026-01-26 — lets a
server hand a host an interactive UI: an HTML document published as a `ui://`
resource with mime type `text/html;profile=mcp-app`, rendered in a sandboxed
iframe that talks JSON-RPC to the host over `postMessage`. A tool links to its
view through `_meta.ui.resourceUri`.

## Why the two models line up

A Smart Panel is a *declarative* description of a view — kind, props, and a
query that re-derives its rows. An MCP App is a *document* that receives its
data by notification (`ui/notifications/tool-result`) rather than fetching it
up front. So a panel transpiles cleanly: its kind and props become a static
template, and the rows arrive at render time exactly as the extension already
intends. One definition, stored once as facts, drives a native GUI
([`datagrout-panels-egui`](https://crates.io/crates/datagrout-panels-egui))
and an MCP host with no second authoring step.

```rust
use datagrout_panels::Panel;
use datagrout_panels_mcp::{to_ui_resource, tool_meta, TranspileOptions};

let resource = to_ui_resource(&panel, &TranspileOptions {
    server: "my-app".into(),
    ..Default::default()
});

// Serve this from `resources/read` for `resource.uri` (ui://my-app/<panel-id>):
let body = resource.to_resource_json();

// And tag the tool whose result carries the rows, so the host opens the view:
let meta = tool_meta(&resource.uri, /* visible_to_model */ true);
```

## Where the panel comes from

A Smart Panel is created on DataGrout by publishing it with the gateway's
`smart_panel.publish` tool; the definition becomes Prolog facts in the
`_panels` namespace of a logic cell, scoped to one account and one hub server.
Reading them back is one `smart_panel.list` call, and
[`datagrout-panels`](https://crates.io/crates/datagrout-panels) parses that
response into the `Panel` values this crate transpiles. A `Panel` built by hand
works just as well, which is how this crate's own tests run.

## Delivering rows

The host sends `ui/notifications/tool-result`; the emitted document reads rows
from `structuredContent`:

- a single panel: `structuredContent.rows`, or the whole `structuredContent`
  if it is an array;
- a dashboard: `structuredContent.panels`, an object keyed by child panel id,
  each value a rows array.

Column headers come from the panel's `columns` prop.

## What it does and does not do

It emits documents and metadata as a pure function of a `Panel`. It does
**not** serve them, register them, or speak MCP — a server does that with
whatever MCP library it already uses. That keeps it testable without a host
and embeddable in any server.

## Security

Panel facts may have been asserted by an agent, so every panel-authored string
is escaped into the document. The emitted view inlines its styles and script
and loads nothing, so `_meta.ui.csp` declares no domains; widening that is the
caller's explicit decision. Display-only panels get no callback path at all;
`TranspileOptions::interactive` (or a form kind) enables one.

## License

`MIT OR Apache-2.0`, at your option.

[SEP-1865]: https://modelcontextprotocol.io/seps/1865-mcp-apps-interactive-user-interfaces-for-mcp
