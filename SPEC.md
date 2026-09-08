# Smart Panel specification

The portable contract. Any language implementing `datagrout-panels` implements
this; renderers are free to differ in everything else. Every statement here was
checked against panels published to a live DataGrout cell — where the schema
documentation and the wire disagreed, the wire won and is recorded below.

---

## 1. Storage

Panels live as Prolog facts in the `_panels` namespace of a DataGrout logic
cell. Publishing is `smart_panel.publish`.

A cell is scoped to one account **and one hub server**, so the panels a caller
can see are those published through the server it is connected to. `_panels` is
a fixed system namespace; the `Namespace` in the facts below is the panel's own
owning namespace, a grouping label chosen by the publisher.

```prolog
panel(Id, Kind, Namespace).
panel_prop(Id, Key, Value).
panel_data(Id, Rows).
panel_source(Id, SourceNamespace, PrologQuery).
field_input(FieldId, DependsOnFieldId).
field_trigger(FieldId, TriggerType, Event).
field_emit(FieldId, EmitType).
```

| fact | meaning |
|---|---|
| `panel/3` | registration for any panel **or** part of one |
| `panel_prop/3` | configuration key/value pairs. Values are typed (§4) |
| `panel_data/2` | static row snapshot |
| `panel_source/3` | live backing. For display kinds a data query; for form fields an invocation goal |
| `field_input/2` | dependency edge — this field needs that field's value |
| `field_trigger/3` | when the field fires. Type: `once \| repeat \| on_event \| asap \| always \| auto`. Event: `submit \| change \| focus \| manual` |
| `field_emit/2` | what happens with the output: `replacement \| trigger \| redirection \| event` |

Those three vocabularies are closed the way kinds are (§2), and an
implementation types them the same way: a total parse, the wire name back, and
an `Unknown` case so a value this port predates is carried through rather than
dropped. The **type** is a cadence the host owes — `once` means at most one run
— while the **event** is what fires the field.

**Identity is `(Namespace, Id)`.** Republishing can leave several identical
`panel/3` rows for the same panel; implementations collapse them. Props, data and
source facts carry no namespace and are keyed by `Id` alone, so an id reused
across namespaces shares them.

## 2. Kinds

**Display:** `bar_chart` `line_chart` `pie_chart` `scatter` `table` `metric`
`gauge` `markdown` `list` `area_chart` `heatmap` `timeline` `funnel` `game` `doc`
`dashboard`

**Form:** `form` `text_input` `textarea` `dropdown` `select` `checkbox` `radio`
`button` `number_input` `date_input` `file_upload` `rich_text`

**Data kinds** — those whose `panel_source` is a live data query rather than a
submit goal, and therefore safe to evaluate on render:

`table` `bar_chart` `line_chart` `pie_chart` `scatter` `metric` `gauge` `list`
`area_chart` `heatmap` `timeline` `funnel`

**Composite kinds** — `dashboard` and `form` — show no data of their own; they
contain other panels (§3).

An unrecognized kind **must not** be an error. Implementations carry an
`Unknown(String)` case and renderers draw a labelled placeholder: a dashboard
that refuses to draw is worse than one that draws incompletely, and servers
publish kinds a given client predates. (`dashboard` itself was such a kind for
this crate until it met a live cell.)

## 3. Composition — the `parent` prop

A container does **not** list its parts. Each part is its own `panel/3` fact
carrying a `parent` prop naming the container:

```prolog
panel(pipeline_dashboard, dashboard, pipeline_pulse).
panel(stage_mix, bar_chart, pipeline_pulse).
panel_prop(stage_mix, parent, pipeline_dashboard).
```

Implementations invert that edge:

- a part whose container is a **form** becomes a *field* of it;
- a part whose container is anything else (a **dashboard**, in practice) becomes
  a *child panel*, itself resolved recursively;
- parts are removed from the top-level result, so callers receive panels, not
  panels-and-their-parts;
- a part whose named parent is absent **stays at the top level** — an orphan you
  can see beats a panel that silently disappears;
- a self-parent or a parent cycle must terminate (the second visit gets no
  parts).

Republishing leaves duplicate `parent` edges. Each part appears under its
container exactly once.

`parent` names an id, not a `(namespace, id)` pair; resolve by id.

## 4. Props

Prop values are **typed**, not strings. Observed on the wire:

| key | type | meaning |
|---|---|---|
| `title` | string | display title; the id is the fallback |
| `description` | string | |
| `published` | **bool** or `"true"`/`"false"` | see below |
| `parent` | string | container id (§3) |
| `columns` | **list of strings** | table column headers |
| `slot` | string | layout hint; server default `"main"` |
| `created_by_agent`, `created_by_agent_name` | string | provenance |
| `doc_ref` | string | `doc_<hex>` reference for `doc` panels |
| `game` | string | game module for `game` panels |
| `store_collection`, `site` | string | where a form's submissions persist |
| `assert_entity` | string | forms that mint an entity per submission |
| `password_hash` | string | gate; never render it |

Implementations keep the value as sent and coerce on read:

- **as text** — strings pass through; numbers and booleans become their text
  form; lists and maps are *not* text and yield nothing;
- **as list** — a JSON list, or tolerantly a comma-separated string;
- **as bool** — `true`/`false` or their string forms.

**`published`** is true only when declared `true` (or `"true"`). An absent prop
means *not published*. This matches the server.

## 5. Row normalization

Values cross the Prolog term boundary in three shapes. All normalize to a list
of cell lists:

| input | result |
|---|---|
| `[["Jan", 4], ["Feb", 6]]` | as-is |
| `[{"month": "Jan", "amt": 4}]` | values in stable key order |
| `"{amount:420,claim:'high risk, escalated'}"` | parsed as a curly term |

Curly-term parsing must track quote and brace depth so commas and colons inside
quoted values or nested braces do not split the term. It is not a general Prolog
parser and does not need to be.

## 6. Derivation, not storage

A panel with a `panel_source` is **derived**: the goal re-runs against the
rulebase every time the panel is read. Implementations prefer the live source
and fall back to the `panel_data` snapshot only when the query yields nothing.

Consequences renderers must respect:

- **Never bake rows into a compiled artifact.** A transpiler emits the panel's
  shape; rows arrive at render time.
- Live queries are capped at **200 rows**. Renderers honor the same ceiling.

## 7. The fetch layer

Fetching is the caller's job, but the parser is shaped around these facts and a
port that ignores them will fail against a real cell:

- **The intended input is `smart_panel.list`.** It returns every panel with
  `id`, `kind`, `namespace`, `props`, `field_ids`, a `data_preview` (first rows
  only) and `source_info`. Implementations accept that shape directly.
- **An undefined predicate is an error, not an empty set.** A cell in which no
  panel has ever declared a source or a field rejects `panel_source(...)` with
  "predicate not defined". Treat that as no rows.
- **Large results are not returned inline.** Past ~48 KB the gateway returns a
  preview plus a `cache_ref`. Retrieve the rows with `prism.paginate`; for a
  `logic.query` result it pages the **solution rows** (`per_page` up to 10,000).
  A whole account's `panel_prop` facts already exceed the inline budget.

## 8. Security

Panel facts may have been asserted by an agent. Every prop, label, and cell is
untrusted input:

- Renderers targeting a markup surface **must** escape all panel-authored text
  in both text and attribute contexts.
- Transpilers **must not** declare CSP domains or permissions by default;
  widening is an explicit caller decision.
- Display-only panels **must not** emit a callback path.
- `password_hash` is never rendered.

## 9. Interaction

Renderers report viewer interaction; they do not act on it. The model layer has
no transport — and the logic does not belong in the client regardless. The two
events:

- **Submit** — a button fired or a form was submitted.
- **ValueChanged** — a field's value changed; dependents are found through
  `field_input` edges.

**Where a submission goes is the host's decision; what it does is defined in
the cell.** On DataGrout a submit runs the panel's goal server-side under the
cell's sandbox, with a bounded timeout and row limit. Fields bind one of two
ways: to the `+` inputs of a rule published with `reactor.expose`, whose mode
contract (`+in:type, -out:type`) shapes the outputs; or into the `panel_source`
goal directly, matching field ids to the goal's Prolog variables and collecting
unbound variables as outputs. A rule body may itself call tools, so a field can
invoke a skill or workflow with nothing in between.

A port therefore implements no cascade logic of its own. Surfacing the edges,
the triggers, the emits and these two events is the whole obligation.

## 10. Renderer conformance

A conforming renderer:

1. Draws every **data kind**, `dashboard`, and every **form kind** — or a
   labelled placeholder. Never an error.
2. Renders a dashboard as a grid of its children, each a full panel.
3. Reads table headers from the `columns` list.
4. Honors the 200-row ceiling.
5. Escapes untrusted text if its surface is markup.
6. Returns interactions rather than performing them.
7. Treats an absent measurement as absent — never substitutes a zero.
