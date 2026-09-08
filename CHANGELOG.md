# Changelog

All notable changes to `datagrout-panels` are recorded here. The project follows
[Semantic Versioning](https://semver.org/); the model crate's public types are
the compatibility surface, and the Rust crates are the reference implementation
other languages port.

## 0.2.0 — 2026-09-08

Types the form-interaction vocabularies, adds a submission helper, and says
what a Smart Panel actually is. 0.1.1 was prepared and never published; its
contents are folded in here.

### Breaking

- `FieldTrigger`'s fields and `FieldEmit` are now typed enums rather than
  strings: `TriggerType` (`Once`, `Repeat`, `OnEvent`, `Asap`, `Always`,
  `Auto`), `TriggerEvent` (`Submit`, `Change`, `Focus`, `Manual`) and
  `FieldEmit` (`Replacement`, `Trigger`, `Redirection`, `Event`). Each follows
  `PanelKind`: a total `parse`, an `as_str` back to the wire name, `FromStr`,
  and an `Unknown(String)` case so a vocabulary this crate predates survives
  the round trip instead of being dropped. Kinds were already typed this way;
  triggers and emits being bare strings meant a host matched `"on_event"` by
  hand and got no compiler help.
- `FieldEmit` was a newtype (`FieldEmit(pub String)`); read it with `as_str`
  or match the enum.

### Added

- `FormState::submission(&Panel)` (`datagrout-panels-egui`) — the
  `{field_id => value}` map a DataGrout form submit expects, with untouched
  fields contributing their declared defaults and buttons excluded. A host had
  to know that `FormState` keys by bare field id and assemble this itself.
- `FieldTrigger::fires_on(&TriggerEvent)` and `Field::fires_on(&TriggerEvent)`
  — whether a field asks to fire on an event. Cadence (`Once` versus
  `Always`) stays the host's obligation, since only the host knows what has
  already run.

### Documentation

- Each crate explains **where panels come from**: they are created on DataGrout
  with `smart_panel.publish`, stored as facts in the `_panels` namespace of a
  logic cell scoped to one account and one hub server, and read back with
  `smart_panel.list`. None of these crates create, fetch or transport panels,
  and 0.1.0 never said so.
- One running example throughout: the `smart_panel.publish` call and the facts
  it compiles to, with each fact's arguments named. 0.1.0 showed a facts block
  and a JSON block that described different panels while claiming to be the
  same one, and wrote the namespace as a Prolog atom in one and a hyphenated
  string in the other.
- **What makes a panel smart**, which 0.1.0 never said: the source is a goal
  that can call rules, so rows are inferred at read time; the definitions are
  facts, so an agent can publish one and `logic.query` can audit it; and a
  form's fields are a dataflow graph of dependencies, triggers and emits whose
  dispatch belongs to the host.
- A copy-pasteable list response, verified as a doctest, so the renderers can
  be tried without a DataGrout account.
- crates.io, docs.rs and CI badges.

## 0.1.0 — 2026-09-07

First release. Three crates:

- **`datagrout-panels`** — the model. Parses `_panels` facts (or a
  `smart_panel.list` response) into a `Panel` tree. No rendering dependency.
- **`datagrout-panels-egui`** — an immediate-mode renderer. Dashboards lay
  children out masonry-style — each child goes to the currently shortest
  column — so a tall table does not leave a hole under a short metric beside
  it; `columns_per_row` sets the column count and order is kept within a
  column.
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
