//! Plugins: a new layer kind without a fork, expressed as data.
//!
//! ShowReel's thirteen content kinds are a closed enum, and for good reason —
//! a layer is a serde tree, not a component, which is what lets one film be
//! written in Rust, loaded from JSON, or emitted over MCP with none of them a
//! special case (see `src/lib.rs` and `docs/remotion-study.md`). A plugin has
//! to add a fourteenth kind *without breaking that*. So a ShowReel plugin is
//! **also data**: a named, parameterised template that expands into the layers
//! the crate already knows how to draw.
//!
//! Why this shape and not the others:
//!
//! - **A Rust trait a plugin crate implements** would be fast and needs no
//!   sandbox, but it forces every plugin author to compile against us, and it
//!   breaks the invariant above — a `Box<dyn Content>` cannot be deserialised
//!   from a `type` tag without a compiled-in registry, and cannot reach the
//!   wasm blob at all unless it was built into it. It splits "a film is JSON"
//!   into "a film is JSON *and* a matching binary".
//! - **A dynamic library loaded at runtime** is the most flexible and the least
//!   safe: arbitrary native code, a versioning surface across an FFI boundary,
//!   and — decisively — **no browser story whatsoever**. `wasm32` has no
//!   `dlopen`; a plugin system built this way works on the desktop and nowhere
//!   else, which is the product-splitting outcome we are told to avoid.
//! - **A declarative template** — this — is limited to what the existing
//!   primitives can already draw, and that is the honest cost. In exchange it
//!   round-trips through JSON, is deterministic and cacheable, needs no sandbox
//!   because it executes nothing, is authorable by an MCP tool like any other
//!   film fragment, and runs **identically in the native and wasm builds**
//!   because expansion is a pure data transform with no I/O of its own.
//!
//! What it can and cannot do, said plainly: a plugin composes the existing
//! vocabulary — a "stat card" is a rounded panel plus a counter plus a label,
//! bound to parameters. It **cannot** invent a mark the primitives cannot draw
//! (a waveform, a QR code); that needs compiled code, and the trade above is
//! why we do not reach for it yet. Parameters substitute directly (`{{value}}`)
//! — arithmetic on them (`{{width}} * 0.5`) is deliberately **not** in this
//! first version; it would want an expression evaluator over named variables,
//! and `src/expr.rs` is single-variable today. Left for a later pass rather
//! than half-built.
//!
//! See `examples/stat_card.plugin.json` for a worked plugin and the README's
//! "Plugins" section for how to write one.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A plugin: a named layer kind, its parameters, and the layers it expands to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    /// The `use` name a `custom` layer refers to it by.
    pub name: String,
    /// Declared parameters — for defaults, for required-ness, and so a reader
    /// (or an MCP tool) can see a plugin's inputs without reverse-engineering
    /// its body. A parameter with no `default` is required.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamDef>,
    /// The template: layers with `{{param}}` placeholders, expanded once per
    /// use. Each entry is an ordinary [`crate::layer::Layer`] in JSON form,
    /// so a plugin body is written and read exactly like the rest of a film.
    pub body: Vec<Value>,
}

/// One declared parameter of a [`Plugin`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    /// The value used when a `custom` layer does not supply one. Absent makes
    /// the parameter required — omitting it is then a loud error, not a blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// A one-line description, for humans and for `showreel info`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

/// How a film brings a plugin into scope: written inline, or referenced by a
/// path resolved through the [`crate::assets::AssetStore`] like any asset.
///
/// Untagged: an object with a `body` is an inline [`Plugin`]; an object with a
/// `file` is a reference. Inline is what the wasm build and a round-trippable
/// film want; a file is what a shareable, reusable plugin is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginRef {
    Inline(Box<Plugin>),
    File {
        /// Path to a `.plugin.json` file, resolved like an image or clip.
        file: String,
    },
}

/// A set of plugins in scope, ready to expand `custom` layers.
#[derive(Debug, Default)]
pub struct Registry {
    plugins: HashMap<String, Plugin>,
}

impl Registry {
    /// Build a registry from a film's declarations, resolving any file
    /// references through `assets`. A duplicate name is an error rather than a
    /// silent last-wins, because which one won would be invisible.
    pub fn build(refs: &[PluginRef], assets: &crate::assets::AssetStore) -> Result<Registry> {
        let mut plugins = HashMap::new();
        for r in refs {
            let plugin = match r {
                PluginRef::Inline(p) => (**p).clone(),
                PluginRef::File { file } => {
                    let path = assets
                        .resolve(file)
                        .with_context(|| format!("plugin file {file:?}"))?;
                    let text = std::fs::read_to_string(&path)
                        .with_context(|| format!("reading plugin file {}", path.display()))?;
                    serde_json::from_str::<Plugin>(&text)
                        .with_context(|| format!("parsing plugin file {}", path.display()))?
                }
            };
            if plugins.contains_key(&plugin.name) {
                bail!("two plugins are both named {:?}", plugin.name);
            }
            plugins.insert(plugin.name.clone(), plugin);
        }
        Ok(Registry { plugins })
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Expand one `custom` use into concrete layers, binding `with` against the
    /// plugin's declared parameters.
    ///
    /// Errors name the plugin and the exact problem — an unknown plugin, a
    /// missing required parameter, an unexpected one (a typo caught, not
    /// ignored), an unresolved `{{placeholder}}`, or a body layer that does not
    /// deserialise once substituted — so a broken plugin fails at load, never
    /// as a silently missing widget.
    pub fn expand(&self, use_: &str, with: &Map<String, Value>) -> Result<Vec<crate::layer::Layer>> {
        let plugin = self
            .plugins
            .get(use_)
            .ok_or_else(|| anyhow::anyhow!("no plugin named {use_:?} is in scope"))?;

        // Bind every declared parameter: the supplied value, or its default,
        // or a loud error if it is required and absent.
        let mut bindings: HashMap<&str, Value> = HashMap::new();
        for p in &plugin.params {
            match with.get(&p.name).or(p.default.as_ref()) {
                Some(v) => {
                    bindings.insert(p.name.as_str(), v.clone());
                }
                None => bail!("plugin {use_:?}: parameter {:?} is required", p.name),
            }
        }
        // Reject an unexpected parameter, so `lable` instead of `label` is an
        // error rather than a value that silently does nothing.
        for k in with.keys() {
            if !plugin.params.iter().any(|p| &p.name == k) {
                let known: Vec<_> = plugin.params.iter().map(|p| format!("{:?}", p.name)).collect();
                bail!(
                    "plugin {use_:?}: no parameter named {k:?}; it takes {}",
                    if known.is_empty() { "no parameters".into() } else { known.join(", ") }
                );
            }
        }

        let mut layers = Vec::with_capacity(plugin.body.len());
        for (i, template) in plugin.body.iter().enumerate() {
            let filled = substitute(template, &bindings, use_)?;
            let layer: crate::layer::Layer = serde_json::from_value(filled)
                .with_context(|| format!("plugin {use_:?}: body layer {i} is not a valid layer"))?;
            layers.push(layer);
        }
        Ok(layers)
    }
}

/// Replace `{{param}}` placeholders throughout a template value.
///
/// A string that is *exactly* one placeholder becomes the bound value with its
/// type intact — `"{{value}}"` against `40000` is the number `40000`, not the
/// string `"40000"` — so a numeric parameter lands in a numeric field. A
/// placeholder embedded in surrounding text interpolates as a string.
fn substitute(v: &Value, bindings: &HashMap<&str, Value>, plugin: &str) -> Result<Value> {
    match v {
        Value::String(s) => substitute_str(s, bindings, plugin),
        Value::Array(a) => Ok(Value::Array(
            a.iter().map(|e| substitute(e, bindings, plugin)).collect::<Result<_>>()?,
        )),
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, val) in o {
                out.insert(k.clone(), substitute(val, bindings, plugin)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn substitute_str(s: &str, bindings: &HashMap<&str, Value>, plugin: &str) -> Result<Value> {
    let trimmed = s.trim();
    // Whole-string placeholder: keep the bound value's type.
    if let Some(name) = trimmed.strip_prefix("{{").and_then(|r| r.strip_suffix("}}")) {
        let name = name.trim();
        if !name.contains("{{") && !name.contains("}}") {
            return bindings
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("plugin {plugin:?}: unknown parameter {{{{{name}}}}}"));
        }
    }
    // Otherwise interpolate each placeholder into the surrounding text.
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("plugin {plugin:?}: unclosed {{{{ in {s:?}"))?;
        let name = after[..end].trim();
        let value = bindings
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("plugin {plugin:?}: unknown parameter {{{{{name}}}}}"))?;
        out.push_str(&value_to_str(value));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(Value::String(out))
}

/// A bound value rendered into interpolated text: a string as itself (not
/// re-quoted), everything else as its JSON.
fn value_to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetStore;

    fn stat_card() -> Plugin {
        serde_json::from_str(
            r##"{
              "name": "stat-card",
              "params": [
                { "name": "value", "default": 0 },
                { "name": "label" },
                { "name": "accent", "default": "#e26" }
              ],
              "body": [
                { "type": "counter", "count": { "from": 0, "to": "{{value}}", "over": 1.2 },
                  "label": "{{label}}" }
              ]
            }"##,
        )
        .unwrap()
    }

    fn registry() -> Registry {
        Registry::build(&[PluginRef::Inline(Box::new(stat_card()))], &AssetStore::new()).unwrap()
    }

    #[test]
    fn a_whole_string_placeholder_keeps_its_type() {
        let mut with = Map::new();
        with.insert("value".into(), Value::from(40000));
        with.insert("label".into(), Value::from("Users"));
        let layers = registry().expand("stat-card", &with).unwrap();
        assert_eq!(layers.len(), 1);
        let crate::layer::Content::Counter { spec, label, .. } = &layers[0].content else {
            panic!("expected a counter");
        };
        // Numeric parameter reached a numeric field, not "40000" the string.
        assert_eq!(spec.to, 40000.0);
        assert_eq!(label.as_deref(), Some("Users"));
    }

    #[test]
    fn a_missing_required_parameter_is_loud() {
        let mut with = Map::new();
        with.insert("value".into(), Value::from(1));
        // `label` has no default and is not supplied.
        let err = registry().expand("stat-card", &with).unwrap_err().to_string();
        assert!(err.contains("label") && err.contains("required"), "{err}");
    }

    #[test]
    fn a_typoed_parameter_is_rejected() {
        let mut with = Map::new();
        with.insert("value".into(), Value::from(1));
        with.insert("label".into(), Value::from("x"));
        with.insert("colour".into(), Value::from("#fff"));
        let err = registry().expand("stat-card", &with).unwrap_err().to_string();
        assert!(err.contains("colour"), "{err}");
    }

    #[test]
    fn an_unknown_plugin_is_named() {
        let err = registry().expand("stat-crd", &Map::new()).unwrap_err().to_string();
        assert!(err.contains("stat-crd"), "{err}");
    }

    #[test]
    fn embedded_placeholders_interpolate() {
        let v = Value::String("Hi {{label}} ({{value}})".into());
        let mut b: HashMap<&str, Value> = HashMap::new();
        b.insert("label", Value::from("Users"));
        b.insert("value", Value::from(40000));
        assert_eq!(substitute(&v, &b, "t").unwrap(), Value::from("Hi Users (40000)"));
    }

    #[test]
    fn a_default_fills_when_absent() {
        let mut with = Map::new();
        with.insert("value".into(), Value::from(5));
        with.insert("label".into(), Value::from("Downloads"));
        // `accent` is defaulted; expansion must succeed without it.
        assert!(registry().expand("stat-card", &with).is_ok());
    }
}
