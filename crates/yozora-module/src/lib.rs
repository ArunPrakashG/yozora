//! The strict, versioned types every Yozora module speaks.
//!
//! Unknown fields are errors everywhere, and styling is limited to semantic
//! [`Tone`]s and [`Size`]s: there is no colour, pixel size, font or radius field.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum rows in one section.
pub const MAX_ROWS_PER_SECTION: usize = 8;
/// Maximum characters in a popover title.
pub const MAX_TITLE_CHARS: usize = 40;
/// Minimum refresh interval in seconds.
pub const MIN_INTERVAL_SECS: u64 = 5;
/// The only manifest schema version understood.
pub const SCHEMA_VERSION: u32 = 1;

/// Node type names exactly as they appear in the serialized `type` tag, plus
/// `badge`, which is not a node but is used inside rows.
pub const COMPONENTS: &[&str] =
    &["section", "row", "toggle", "slider", "progress", "buttons", "empty", "note", "badge"];

/// Schema error. The message keeps serde's field names (`unknown field `x``).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Semantic colour role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    fn is_default(&self) -> bool {
        *self == Tone::Neutral
    }
}

/// Semantic size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Size {
    Compact,
    #[default]
    Regular,
}

impl Size {
    fn is_default(&self) -> bool {
        *self == Size::Regular
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// How a module is refreshed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Refresh {
    /// Re-run every N seconds.
    Interval(u64),
    /// The module pushes updates itself; must be `true`.
    Push(bool),
}

/// A declared action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDecl {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub destructive: bool,
}

/// `module.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub name: String,
    pub title: String,
    pub version: String,
    pub icon: String,
    pub refresh: Refresh,
    pub exec: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionDecl>,
}

impl Manifest {
    /// Parse and validate a `module.toml`.
    pub fn parse(src: &str) -> Result<Manifest, Error> {
        let m: Manifest = toml::from_str(src).map_err(|e| Error(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.schema != SCHEMA_VERSION {
            return Err(Error(format!("unsupported schema `{}`, expected {SCHEMA_VERSION}", self.schema)));
        }
        let ok_name = !self.name.is_empty()
            && self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok_name {
            return Err(Error(format!("invalid `name` {:?}: use lowercase letters, digits and hyphens", self.name)));
        }
        match self.refresh {
            Refresh::Interval(n) if n < MIN_INTERVAL_SECS => {
                return Err(Error(format!("`refresh.interval` is {n}, minimum is {MIN_INTERVAL_SECS} seconds")));
            }
            Refresh::Push(false) => {
                return Err(Error("`refresh.push` must be true".into()));
            }
            _ => {}
        }
        Ok(())
    }
}

/// The bar item a module shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarItem {
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<u32>,
    #[serde(default, skip_serializing_if = "Tone::is_default")]
    pub tone: Tone,
}

/// A small tone-coloured label used inside rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Badge {
    pub text: String,
    #[serde(default, skip_serializing_if = "Tone::is_default")]
    pub tone: Tone,
}

/// A button in a `buttons` node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Button {
    pub label: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub primary: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub destructive: bool,
}

/// A popover view-tree node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Node {
    Section {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        rows: Vec<Node>,
    },
    Row {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        badge: Option<Badge>,
        #[serde(default, skip_serializing_if = "Tone::is_default")]
        tone: Tone,
        #[serde(default, skip_serializing_if = "Size::is_default")]
        size: Size,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },
    Toggle {
        title: String,
        on: bool,
        action: String,
    },
    Slider {
        title: String,
        value: f32,
        action: String,
    },
    Progress {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        value: f32,
    },
    Buttons {
        items: Vec<Button>,
    },
    Empty {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Note {
        text: String,
    },
}

/// The popover content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Popover {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub body: Vec<Node>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_note: Option<String>,
}

impl Popover {
    /// Check the documented limits; returns every problem found.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if self.title.chars().count() > MAX_TITLE_CHARS {
            errs.push(format!("title is longer than {MAX_TITLE_CHARS} characters"));
        }
        for n in &self.body {
            validate_node(n, &mut errs);
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

fn check_action(kind: &str, action: &str, errs: &mut Vec<String>) {
    if action.is_empty() {
        errs.push(format!("{kind} has an empty action"));
    }
}

fn check_unit(kind: &str, v: f32, errs: &mut Vec<String>) {
    if !(0.0..=1.0).contains(&v) {
        errs.push(format!("{kind} value {v} is outside 0..=1"));
    }
}

fn validate_node(node: &Node, errs: &mut Vec<String>) {
    match node {
        Node::Section { rows, .. } => {
            if rows.len() > MAX_ROWS_PER_SECTION {
                errs.push(format!("section has {} rows, maximum is {MAX_ROWS_PER_SECTION}", rows.len()));
            }
            for r in rows {
                validate_node(r, errs);
            }
        }
        Node::Row { action, .. } => {
            if let Some(a) = action {
                check_action("row", a, errs);
            }
        }
        Node::Toggle { action, .. } => check_action("toggle", action, errs),
        Node::Slider { value, action, .. } => {
            check_unit("slider", *value, errs);
            check_action("slider", action, errs);
        }
        Node::Progress { value, .. } => check_unit("progress", *value, errs),
        Node::Buttons { items } => {
            for b in items {
                check_action("button", &b.action, errs);
            }
        }
        Node::Empty { .. } | Node::Note { .. } => {}
    }
}

/// Module to host: one JSON object per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub bar: BarItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popover: Option<Popover>,
}

impl Update {
    /// Parse one JSON line.
    pub fn parse_line(line: &str) -> Result<Update, Error> {
        serde_json::from_str(line).map_err(|e| Error(e.to_string()))
    }
}

/// Host to module: one JSON object per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostMessage {
    Action {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
    },
    Refresh,
}
