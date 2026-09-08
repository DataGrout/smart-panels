# datagrout-panels-egui

Render [DataGrout](https://datagrout.ai) Smart Panels natively with
[egui](https://github.com/emilk/egui).

Takes a `Panel` from [`datagrout-panels`](https://crates.io/crates/datagrout-panels)
and draws it. One function per kind, dispatched by `render_panel`. Every
renderer is pure: it reads the panel and paints. Nothing here fetches, caches,
mutates, or speaks to a gateway — and it must not grow a transport, because the
same panel has to be renderable by a host that reaches its cell over MCP, over
a local proxy, or not at all.

```rust
use datagrout_panels::Panel;
use datagrout_panels_egui::render_panel;

let panels = Panel::all_from_list(&list_response);

egui::CentralPanel::default().show(ctx, |ui| {
    for panel in &panels {
        render_panel(ui, panel);
    }
});
```

## Why immediate mode fits

egui redraws from state every frame with no retained widget tree. A Smart
Panel re-derives its rows from the rulebase on every read with no retained
DOM. They are the same architecture, so rendering one with the other costs no
reconciliation — there is no element tree to diff panel facts into.

## Kinds

| kind | drawn as |
|---|---|
| `metric` | one large number |
| `gauge` | a bar against `min`/`max` props |
| `table` | rows under the `columns` prop as headers |
| `list` | a bulleted list |
| `bar_chart`, `line_chart`, `area_chart`, `heatmap` | painted directly, no plotting dependency |
| `markdown`, `doc` | the `body` prop as plain text (special-case before calling if you want rich text) |
| `dashboard` | its children in columns, masonry-style: each child goes to the currently shortest column, so a tall table leaves no hole beside a short metric; `columns_per_row` sets the column count |
| form kinds | widgets that report through `PanelAction` |
| anything else | a labelled placeholder — an unknown kind never panics |

## Forms

Immediate mode means widgets hold nothing between frames, so form values live
in a `FormState` the caller keeps:

```rust
use datagrout_panels_egui::{render_panel_with_state, FormState, PanelAction};

let mut state = FormState::default();   // keep this across frames

for action in render_panel_with_state(ui, &panel, &mut state) {
    match action {
        PanelAction::Submit { panel_id, field_id } => { /* run the field's goal */ }
        PanelAction::ValueChanged { panel_id, field_id, value } => { /* cascade */ }
    }
}
```

Actions are *returned*, not executed. Dispatching a submit means running a
goal on a gateway, which is the host's decision and the host's transport.

## Testing without a window

egui renders headlessly, so panel rendering is unit-testable:

```rust
let ctx = egui::Context::default();
let _ = ctx.run(egui::RawInput::default(), |ctx| {
    egui::CentralPanel::default().show(ctx, |ui| {
        render_panel(ui, &panel);
    });
});
```

This crate's own tests render every kind that way.

## License

`MIT OR Apache-2.0`, at your option.
