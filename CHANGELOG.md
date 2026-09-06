# Changelog

All notable changes to `datagrout-panels` are recorded here. The project follows
[Semantic Versioning](https://semver.org/); the model crate's public types are
the compatibility surface, and the Rust crates are the reference implementation
other languages port.

## [Unreleased]

### 0.1.0 — first release

Three crates:

- **`datagrout-panels`** — the model. Parses `_panels` facts (or a
  `smart_panel.list` response) into a `Panel` tree. No rendering dependency.
- **`datagrout-panels-egui`** — an immediate-mode renderer.
- **`datagrout-panels-mcp`** — a transpiler to MCP Apps (SEP-1865) `ui://`
  resources.

Contract points fixed against a live gateway before this release, each of which
a schema-derived fixture had got wrong:

- A panel's parts are discovered by the **child's `parent` prop**
  (`panel_prop(Child, parent, Container)`), not by a list on the container.
  Form fields and dashboard children use the same mechanism.
- `dashboard` is a display kind: a grid of the panels whose `parent` names it.
- Props are **typed values**, not strings — `columns` is a list, `published` a
  boolean. `Panel::props` is `BTreeMap<String, serde_json::Value>` with
  coercing accessors (`prop_str`, `prop_list`, `prop_bool`, `prop_f64`).
- `published` is true only when declared `true` / `"true"`, matching the
  server; absent means unpublished.
- Panel identity is `(namespace, id)`. Republishing can leave duplicate
  registration rows and `parent` edges; both collapse.
- `smart_panel.list` output is accepted directly (`Panel::all_from_list`).
  Its `data_preview` is a preview, not the full row set.

Fetch-layer facts documented in `SPEC.md` (they belong to the caller, but the
parser is shaped around them): an undefined predicate is an error to be treated
as an empty set; results past ~48 KB come back as a `cache_ref` to be paged with
`prism.paginate`, which pages `logic.query` results by solution row.
