//! Declarative settings schemas. Settings renders a generic page from one of these for any app
//! it has no built-in page for.
//!
//! ```toml
//! [[section]]
//! title = "Layout"
//!
//! [[section.field]]
//! key     = "bar.height"   # dotted path into the app's config file
//! kind    = "enum"
//! options = [30, 34, 38]
//! default = 34
//! label   = "Height"
//! help    = "Icons stay 16 px at every height."
//! ```

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Bool,
    Enum,
    Int,
    Float,
    String,
    List,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Field {
    pub key: String,
    pub kind: FieldKind,
    pub label: String,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// Choices for `enum`.
    #[serde(default)]
    pub options: Vec<toml::Value>,
    /// Display names for `options`, same order.
    #[serde(default)]
    pub option_labels: Vec<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
}

impl Field {
    pub fn option_label(&self, index: usize) -> String {
        self.option_labels
            .get(index)
            .cloned()
            .unwrap_or_else(|| display_value(&self.options[index]))
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Section {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub field: Vec<Field>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Schema {
    #[serde(default)]
    pub section: Vec<Section>,
}

impl Schema {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let schema: Schema = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e.to_string().trim()))?;
        for field in schema.section.iter().flat_map(|s| &s.field) {
            anyhow::ensure!(
                field.kind != FieldKind::Enum || !field.options.is_empty(),
                "{}: enum field `{}` has no options",
                path.display(),
                field.key
            );
        }
        Ok(schema)
    }
}

/// A value as a person would read it: strings without quotes.
pub fn display_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let text = r#"
[[section]]
title = "Layout"

[[section.field]]
key     = "bar.height"
kind    = "enum"
options = [30, 34, 38]
default = 34
label   = "Height"
help    = "Icons stay 16 px at every height."
"#;
        let schema: Schema = toml::from_str(text).unwrap();
        let field = &schema.section[0].field[0];
        assert_eq!(field.kind, FieldKind::Enum);
        assert_eq!(field.option_label(1), "34");
    }
}
