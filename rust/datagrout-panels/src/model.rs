//! The panel model — a typed view of the `_panels` fact schema.
//!
//! Kind lists mirror DataGrout's `smart_panel.publish` (`@display_kinds` /
//! `@form_kinds`). Keeping them in sync is what lets [`PanelKind::parse`] be
//! total rather than lossy; a kind this crate predates still parses, as
//! [`PanelKind::Unknown`].
//!
//! # Composition
//!
//! A panel's parts — a form's fields, a dashboard's child panels — are not
//! listed on the parent. Each part is its own `panel/3` fact carrying a
//! `parent` prop that names the container:
//!
//! ```prolog
//! panel(pipeline_dashboard, dashboard, pipeline_pulse).
//! panel(stage_mix, bar_chart, pipeline_pulse).
//! panel_prop(stage_mix, parent, pipeline_dashboard).
//! ```
//!
//! The parser inverts that edge, so a resolved [`Panel`] carries its
//! [`children`](Panel::children) (for dashboards) or [`fields`](Panel::fields)
//! (for forms) and the parts do not appear at the top level.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A panel kind. Display kinds render data; form kinds collect it.
///
/// `Unknown` is deliberate: DataGrout may publish a kind this crate predates,
/// and a renderer that panics on an unrecognized panel is worse than one that
/// draws a labelled placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelKind {
    // ── display ──────────────────────────────────────────────────────────
    BarChart,
    LineChart,
    PieChart,
    Scatter,
    Table,
    Metric,
    Gauge,
    Markdown,
    List,
    AreaChart,
    Heatmap,
    Timeline,
    Funnel,
    Game,
    Doc,
    /// A grid of the data panels whose `parent` prop names it.
    Dashboard,
    // ── form ─────────────────────────────────────────────────────────────
    Form,
    TextInput,
    TextArea,
    Dropdown,
    Select,
    Checkbox,
    Radio,
    Button,
    NumberInput,
    DateInput,
    FileUpload,
    RichText,
    /// A kind this crate does not know about. Rendered as a placeholder.
    Unknown(String),
}

impl std::str::FromStr for PanelKind {
    type Err = std::convert::Infallible;

    /// Every string parses; an unrecognized one becomes [`PanelKind::Unknown`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

impl PanelKind {
    /// Parse a wire name such as `"bar_chart"`. Total: unknown names become
    /// [`PanelKind::Unknown`] rather than failing.
    pub fn parse(s: &str) -> Self {
        match s {
            "bar_chart" => Self::BarChart,
            "line_chart" => Self::LineChart,
            "pie_chart" => Self::PieChart,
            "scatter" => Self::Scatter,
            "table" => Self::Table,
            "metric" => Self::Metric,
            "gauge" => Self::Gauge,
            "markdown" => Self::Markdown,
            "list" => Self::List,
            "area_chart" => Self::AreaChart,
            "heatmap" => Self::Heatmap,
            "timeline" => Self::Timeline,
            "funnel" => Self::Funnel,
            "game" => Self::Game,
            "doc" => Self::Doc,
            "dashboard" => Self::Dashboard,
            "form" => Self::Form,
            "text_input" => Self::TextInput,
            "textarea" => Self::TextArea,
            "dropdown" => Self::Dropdown,
            "select" => Self::Select,
            "checkbox" => Self::Checkbox,
            "radio" => Self::Radio,
            "button" => Self::Button,
            "number_input" => Self::NumberInput,
            "date_input" => Self::DateInput,
            "file_upload" => Self::FileUpload,
            "rich_text" => Self::RichText,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// The wire name, e.g. `"bar_chart"`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::BarChart => "bar_chart",
            Self::LineChart => "line_chart",
            Self::PieChart => "pie_chart",
            Self::Scatter => "scatter",
            Self::Table => "table",
            Self::Metric => "metric",
            Self::Gauge => "gauge",
            Self::Markdown => "markdown",
            Self::List => "list",
            Self::AreaChart => "area_chart",
            Self::Heatmap => "heatmap",
            Self::Timeline => "timeline",
            Self::Funnel => "funnel",
            Self::Game => "game",
            Self::Doc => "doc",
            Self::Dashboard => "dashboard",
            Self::Form => "form",
            Self::TextInput => "text_input",
            Self::TextArea => "textarea",
            Self::Dropdown => "dropdown",
            Self::Select => "select",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::Button => "button",
            Self::NumberInput => "number_input",
            Self::DateInput => "date_input",
            Self::FileUpload => "file_upload",
            Self::RichText => "rich_text",
            Self::Unknown(s) => s,
        }
    }

    /// Kinds whose `panel_source` is a live DATA query rather than a submit
    /// goal. Matches DataGrout's `PanelData.data_kinds/0` — the distinction
    /// decides whether a source is safe to evaluate on render.
    pub fn is_data_kind(&self) -> bool {
        matches!(
            self,
            Self::Table
                | Self::BarChart
                | Self::LineChart
                | Self::PieChart
                | Self::Scatter
                | Self::Metric
                | Self::Gauge
                | Self::List
                | Self::AreaChart
                | Self::Heatmap
                | Self::Timeline
                | Self::Funnel
        )
    }

    pub fn is_form_kind(&self) -> bool {
        matches!(
            self,
            Self::Form
                | Self::TextInput
                | Self::TextArea
                | Self::Dropdown
                | Self::Select
                | Self::Checkbox
                | Self::Radio
                | Self::Button
                | Self::NumberInput
                | Self::DateInput
                | Self::FileUpload
                | Self::RichText
        )
    }

    /// Kinds that compose other panels rather than showing data of their own.
    pub fn is_composite(&self) -> bool {
        matches!(self, Self::Dashboard | Self::Form)
    }
}

/// How often a field's goal may run — the second argument of
/// `field_trigger(FieldId, TriggerType, Event)`.
///
/// The cadence is the *host's* obligation, not the renderer's: `Once` means at
/// most one run per form, `Repeat` and `Always` mean every occurrence, and
/// `Auto` leaves it to the host's own policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerType {
    Once,
    Repeat,
    OnEvent,
    Asap,
    Always,
    Auto,
    /// A cadence this crate predates. Carried through rather than dropped.
    Unknown(String),
}

impl std::str::FromStr for TriggerType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

impl TriggerType {
    /// Parse a wire name such as `"on_event"`. Total: an unrecognized name
    /// becomes [`TriggerType::Unknown`].
    pub fn parse(s: &str) -> Self {
        match s {
            "once" => Self::Once,
            "repeat" => Self::Repeat,
            "on_event" => Self::OnEvent,
            "asap" => Self::Asap,
            "always" => Self::Always,
            "auto" => Self::Auto,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// The wire name, e.g. `"on_event"`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Once => "once",
            Self::Repeat => "repeat",
            Self::OnEvent => "on_event",
            Self::Asap => "asap",
            Self::Always => "always",
            Self::Auto => "auto",
            Self::Unknown(s) => s,
        }
    }
}

/// What fires a field's goal — the third argument of `field_trigger/3`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerEvent {
    Submit,
    Change,
    Focus,
    Manual,
    /// An event this crate predates.
    Unknown(String),
}

impl std::str::FromStr for TriggerEvent {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

impl TriggerEvent {
    /// Parse a wire name such as `"change"`. Total: an unrecognized name
    /// becomes [`TriggerEvent::Unknown`].
    pub fn parse(s: &str) -> Self {
        match s {
            "submit" => Self::Submit,
            "change" => Self::Change,
            "focus" => Self::Focus,
            "manual" => Self::Manual,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// The wire name, e.g. `"change"`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Submit => "submit",
            Self::Change => "change",
            Self::Focus => "focus",
            Self::Manual => "manual",
            Self::Unknown(s) => s,
        }
    }
}

/// When a form field fires. From `field_trigger(FieldId, TriggerType, Event)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldTrigger {
    pub trigger_type: TriggerType,
    pub event: TriggerEvent,
}

impl FieldTrigger {
    /// Whether this trigger names `event` as what fires it.
    ///
    /// Says nothing about the cadence — a `Once` trigger that fires on
    /// `Change` still answers `true` here, and honouring "once" is the host's
    /// job, since only the host knows what has already run.
    pub fn fires_on(&self, event: &TriggerEvent) -> bool {
        &self.event == event
    }
}

/// What happens with a field's output. From `field_emit(FieldId, EmitType)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldEmit {
    /// The output replaces the field's own value.
    Replacement,
    /// The output fires dependent fields.
    Trigger,
    /// The output is a destination to send the viewer to.
    Redirection,
    /// The output is an event for the host to route.
    Event,
    /// An emit kind this crate predates.
    Unknown(String),
}

impl std::str::FromStr for FieldEmit {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

impl FieldEmit {
    /// Parse a wire name such as `"replacement"`. Total: an unrecognized name
    /// becomes [`FieldEmit::Unknown`].
    pub fn parse(s: &str) -> Self {
        match s {
            "replacement" => Self::Replacement,
            "trigger" => Self::Trigger,
            "redirection" => Self::Redirection,
            "event" => Self::Event,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// The wire name, e.g. `"replacement"`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Replacement => "replacement",
            Self::Trigger => "trigger",
            Self::Redirection => "redirection",
            Self::Event => "event",
            Self::Unknown(s) => s,
        }
    }
}

/// A live query backing — `panel_source(Id, SourceNamespace, PrologQuery)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSource {
    pub namespace: String,
    pub query: String,
}

/// Prop values as they cross the wire — strings, booleans, numbers, lists.
///
/// Props are **not** all strings: `columns` is a list and `published` a
/// boolean in real cells. Storing them as [`Value`] keeps what the server sent;
/// the accessors on [`Panel`] and [`Field`] do the coercion a renderer wants.
pub type Props = BTreeMap<String, Value>;

/// Read a prop as text, coercing scalars the way the server does.
///
/// Mirrors `PanelData.prop_value/1`: strings pass through, numbers and
/// booleans become their text form, lists and maps are not text and yield
/// `None` — a renderer that wants a list asks [`prop_list`].
pub fn prop_str(props: &Props, key: &str) -> Option<String> {
    match props.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Read a prop as a list of strings.
///
/// Accepts a JSON list (the wire form for `columns`) and, tolerantly, a
/// comma-separated string.
pub fn prop_list(props: &Props, key: &str) -> Vec<String> {
    match props.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Read a prop as a boolean. Accepts `true`/`false` and their string forms,
/// which is how `published` arrives depending on the publishing client.
pub fn prop_bool(props: &Props, key: &str) -> Option<bool> {
    match props.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Read a prop as a number, from a JSON number or numeric string.
pub fn prop_f64(props: &Props, key: &str) -> Option<f64> {
    match props.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A form field: a part of a `form` panel, linked to it by the field's own
/// `parent` prop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub kind: PanelKind,
    pub props: Props,
    /// `field_input(FieldId, DependsOnFieldId)` — dependency edges.
    pub inputs: Vec<String>,
    pub trigger: Option<FieldTrigger>,
    pub emit: Option<FieldEmit>,
    pub source: Option<PanelSource>,
}

impl Field {
    pub fn label(&self) -> String {
        prop_str(&self.props, "label").unwrap_or_else(|| self.id.clone())
    }

    pub fn placeholder(&self) -> String {
        prop_str(&self.props, "placeholder").unwrap_or_default()
    }

    pub fn required(&self) -> bool {
        prop_bool(&self.props, "required").unwrap_or(false)
    }

    /// Default value, if the field declares one.
    pub fn default_value(&self) -> Option<String> {
        prop_str(&self.props, "default")
    }

    /// Whether this field asks to fire its goal on `event`.
    ///
    /// A field with no `field_trigger` fact declares no cadence of its own, so
    /// this is `false` for it: its value travels with the form's submit rather
    /// than firing anything independently.
    pub fn fires_on(&self, event: &TriggerEvent) -> bool {
        self.trigger
            .as_ref()
            .is_some_and(|trigger| trigger.fires_on(event))
    }
}

/// A resolved panel: definition facts plus whatever rows were loaded for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub kind: PanelKind,
    pub namespace: String,
    /// `panel_prop(Id, Key, Value)` pairs, values as sent.
    pub props: Props,
    /// Rows: either the live `panel_source` result or the `panel_data`
    /// snapshot. Resolution happens outside this crate — see [`PanelSource`].
    pub rows: Vec<Vec<Value>>,
    pub source: Option<PanelSource>,
    /// Form fields — parts of a `form` panel.
    pub fields: Vec<Field>,
    /// Child panels — parts of a `dashboard` (or any non-form container).
    pub children: Vec<Panel>,
    /// Whether the publisher marked this panel published.
    ///
    /// Matches the server: `published` must be `true` (or `"true"`); an
    /// absent prop means **not** published.
    pub published: bool,
}

impl Panel {
    pub fn title(&self) -> String {
        prop_str(&self.props, "title").unwrap_or_else(|| self.id.clone())
    }

    pub fn description(&self) -> Option<String> {
        prop_str(&self.props, "description")
    }

    /// Layout slot hint. The server defaults this to `"main"`.
    pub fn slot(&self) -> String {
        prop_str(&self.props, "slot").unwrap_or_else(|| "main".to_string())
    }

    /// The container this panel is a part of, if any.
    pub fn parent(&self) -> Option<String> {
        prop_str(&self.props, "parent")
    }

    /// Column headers for `table` panels — the `columns` prop, a list.
    pub fn columns(&self) -> Vec<String> {
        prop_list(&self.props, "columns")
    }

    /// Read a prop as text. See [`prop_str`].
    pub fn prop_str(&self, key: &str) -> Option<String> {
        prop_str(&self.props, key)
    }

    /// Read a prop as a list. See [`prop_list`].
    pub fn prop_list(&self, key: &str) -> Vec<String> {
        prop_list(&self.props, key)
    }

    /// Read a prop as a boolean. See [`prop_bool`].
    pub fn prop_bool(&self, key: &str) -> Option<bool> {
        prop_bool(&self.props, key)
    }

    /// Read a prop as a number. See [`prop_f64`].
    pub fn prop_f64(&self, key: &str) -> Option<f64> {
        prop_f64(&self.props, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(pairs: &[(&str, Value)]) -> Props {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn every_known_kind_round_trips_through_its_wire_name() {
        for name in [
            "bar_chart",
            "line_chart",
            "pie_chart",
            "scatter",
            "table",
            "metric",
            "gauge",
            "markdown",
            "list",
            "area_chart",
            "heatmap",
            "timeline",
            "funnel",
            "game",
            "doc",
            "dashboard",
            "form",
            "text_input",
            "textarea",
            "dropdown",
            "select",
            "checkbox",
            "radio",
            "button",
            "number_input",
            "date_input",
            "file_upload",
            "rich_text",
        ] {
            let kind = PanelKind::parse(name);
            assert_eq!(name.parse::<PanelKind>().unwrap(), kind);
            assert!(
                !matches!(kind, PanelKind::Unknown(_)),
                "{name} should be a known kind"
            );
            assert_eq!(kind.as_str(), name);
        }
    }

    #[test]
    fn dashboard_is_composite_not_data() {
        assert!(PanelKind::Dashboard.is_composite());
        assert!(!PanelKind::Dashboard.is_data_kind());
        assert!(!PanelKind::Dashboard.is_form_kind());
    }

    #[test]
    fn prop_str_coerces_scalars_and_refuses_lists() {
        let p = props(&[
            ("title", json!("Pipeline")),
            ("limit", json!(40)),
            ("published", json!(true)),
            ("columns", json!(["a", "b"])),
        ]);
        assert_eq!(prop_str(&p, "title").as_deref(), Some("Pipeline"));
        assert_eq!(prop_str(&p, "limit").as_deref(), Some("40"));
        assert_eq!(prop_str(&p, "published").as_deref(), Some("true"));
        // A list is not text; a caller that wants it asks prop_list.
        assert_eq!(prop_str(&p, "columns"), None);
    }

    #[test]
    fn prop_list_reads_a_json_list_or_a_comma_string() {
        let p = props(&[
            ("columns", json!(["Invoice", "Days", 3])),
            ("legacy", json!("a, b ,c")),
        ]);
        assert_eq!(prop_list(&p, "columns"), vec!["Invoice", "Days", "3"]);
        assert_eq!(prop_list(&p, "legacy"), vec!["a", "b", "c"]);
        assert!(prop_list(&p, "missing").is_empty());
    }

    #[test]
    fn prop_bool_accepts_both_wire_forms() {
        let p = props(&[
            ("a", json!(true)),
            ("b", json!("false")),
            ("c", json!("maybe")),
        ]);
        assert_eq!(prop_bool(&p, "a"), Some(true));
        assert_eq!(prop_bool(&p, "b"), Some(false));
        assert_eq!(prop_bool(&p, "c"), None);
    }

    #[test]
    fn slot_defaults_to_main_like_the_server() {
        let panel = Panel {
            id: "x".into(),
            kind: PanelKind::Metric,
            namespace: "ns".into(),
            props: Props::new(),
            rows: vec![],
            source: None,
            fields: vec![],
            children: vec![],
            published: false,
        };
        assert_eq!(panel.slot(), "main");
        assert_eq!(panel.title(), "x");
    }
}
