//! The Publish JSON, built only from the Registry (FR-11, FR-61, AD-7):
//! one command object per base node and per context menu, in Registry
//! order. Set metadata appears exactly as declared and unset fields are
//! omitted. Localizations from the merged locale catalogs (FR-41) follow
//! `name` and `description` as `name_localizations` /
//! `description_localizations` objects keyed by locale code; an empty one
//! is omitted, so an App without catalogs sends none.

use noctis_rules::command::{ChoiceValue, NumericValue, OptionDecl};
use serde_json::{Map, Value, json};

use crate::metadata::CommandMetadata;
use crate::registry::commands::{CommandNode, OptionTexts, ScopeCommands, Variants};

/// Every command object of one scope: the chat-input bases, then the
/// context menus.
pub(crate) fn local(scope: &ScopeCommands) -> Vec<Value> {
    local_kept(scope, &|_| true)
}

/// [`local`] with only the handlers whose Module `keep` accepts: a node
/// without a kept handler left under it (a base or group) is dropped.
pub(crate) fn local_kept(scope: &ScopeCommands, keep: &dyn Fn(&'static str) -> bool) -> Vec<Value> {
    let bases = scope
        .bases
        .iter()
        .filter(|base| kept(base, keep))
        .map(|base| {
            let mut object = node(base, 1, keep);
            add_metadata(&mut object, &base.metadata);
            Value::Object(object)
        });
    let menus = scope
        .menus()
        .filter(|entry| keep(entry.module))
        .map(|entry| {
            let handler = &entry.handler;
            let mut object = Map::new();
            object.insert("type".into(), json!(handler.kind().value()));
            object.insert("name".into(), json!(handler.menu_name()));
            add_localizations(&mut object, "name_localizations", &entry.name_localizations);
            add_metadata(&mut object, handler.metadata());
            Value::Object(object)
        });
    bases.chain(menus).collect()
}

/// Inserts `field` as a locale-code object, unless `variants` is empty.
fn add_localizations(object: &mut Map<String, Value>, field: &str, variants: &Variants) {
    let localized = variants.localized();
    if localized.is_empty() {
        return;
    }
    let map: Map<String, Value> = localized
        .into_iter()
        .map(|(locale, value)| (locale.as_str().to_owned(), json!(value)))
        .collect();
    object.insert(field.into(), Value::Object(map));
}

/// Removes `contexts` and `integration_types` (guild commands cannot carry
/// them, META-03).
pub(super) fn strip_install(command: &mut Value) {
    if let Value::Object(object) = command {
        object.remove("contexts");
        object.remove("integration_types");
    }
}

/// Whether `node` has a handler `keep` accepts (itself or below it).
fn kept(node: &CommandNode, keep: &dyn Fn(&'static str) -> bool) -> bool {
    if node.children.is_empty() {
        node.handler.as_ref().is_none_or(|entry| keep(entry.module))
    } else {
        node.children.iter().any(|child| kept(child, keep))
    }
}

/// A chat-input node as a command (`kind` 1 at the base), subcommand (1) or
/// group (2), with only its [`kept`] children. It always carries `options`
/// (FR-61).
fn node(node: &CommandNode, kind: u8, keep: &dyn Fn(&'static str) -> bool) -> Map<String, Value> {
    let options: Vec<Value> = if node.children.is_empty() {
        let texts = node.options.iter().map(Some).chain(std::iter::repeat(None));
        node.handler
            .iter()
            .flat_map(|entry| entry.handler.options())
            .zip(texts)
            .map(|(decl, texts)| option(decl, texts))
            .collect()
    } else {
        node.children
            .iter()
            .filter(|child| kept(child, keep))
            .map(|child| {
                let kind = if child.children.is_empty() { 1 } else { 2 };
                Value::Object(self::node(child, kind, keep))
            })
            .collect()
    };
    let mut object = Map::new();
    object.insert("type".into(), json!(kind));
    object.insert("name".into(), json!(node.word));
    add_localizations(&mut object, "name_localizations", &node.texts.name);
    object.insert("description".into(), json!(node.description));
    add_localizations(
        &mut object,
        "description_localizations",
        &node.texts.description,
    );
    object.insert("options".into(), Value::Array(options));
    object
}

/// Adds each set metadata field.
fn add_metadata(object: &mut Map<String, Value>, metadata: &CommandMetadata) {
    if let Some(permissions) = metadata.default_member_permissions {
        object.insert(
            "default_member_permissions".into(),
            json!(permissions.bits().to_string()),
        );
    }
    if let Some(contexts) = metadata.contexts {
        let values: Vec<u8> = contexts.iter().map(|c| c.value()).collect();
        object.insert("contexts".into(), json!(values));
    }
    if let Some(types) = metadata.integration_types {
        let values: Vec<u8> = types.iter().map(|t| t.value()).collect();
        object.insert("integration_types".into(), json!(values));
    }
    if let Some(nsfw) = metadata.nsfw {
        object.insert("nsfw".into(), json!(nsfw));
    }
}

/// A JSON integer for `Int`, a float otherwise.
fn numeric(value: NumericValue) -> Value {
    match value {
        NumericValue::Int(v) => json!(v),
        NumericValue::Float(v) => json!(v),
    }
}

/// One option object: `type`, `name` and its localizations, `description`
/// and its localizations, then each set field.
fn option(decl: &OptionDecl<'_>, texts: Option<&OptionTexts>) -> Value {
    let mut object = Map::new();
    object.insert("type".into(), json!(decl.kind.value()));
    object.insert("name".into(), json!(decl.name));
    if let Some(texts) = texts {
        add_localizations(&mut object, "name_localizations", &texts.name);
    }
    object.insert("description".into(), json!(decl.description.default));
    if let Some(texts) = texts {
        add_localizations(&mut object, "description_localizations", &texts.description);
    }
    if decl.required {
        object.insert("required".into(), json!(true));
    }
    if !decl.choices.is_empty() {
        let choices: Vec<Value> = decl
            .choices
            .iter()
            .enumerate()
            .map(|(j, choice)| {
                let value = match choice.value {
                    ChoiceValue::Str(v) => json!(v),
                    ChoiceValue::Int(v) => json!(v),
                    ChoiceValue::Float(v) => json!(v),
                };
                let mut object = Map::new();
                object.insert("name".into(), json!(choice.name.default));
                if let Some(names) = texts.and_then(|t| t.choices.get(j)) {
                    add_localizations(&mut object, "name_localizations", names);
                }
                object.insert("value".into(), value);
                Value::Object(object)
            })
            .collect();
        object.insert("choices".into(), Value::Array(choices));
    }
    if let Some(min) = decl.min_value {
        object.insert("min_value".into(), numeric(min));
    }
    if let Some(max) = decl.max_value {
        object.insert("max_value".into(), numeric(max));
    }
    if let Some(min) = decl.min_length {
        object.insert("min_length".into(), json!(min));
    }
    if let Some(max) = decl.max_length {
        object.insert("max_length".into(), json!(max));
    }
    if !decl.channel_types.is_empty() {
        object.insert("channel_types".into(), json!(decl.channel_types));
    }
    if decl.autocomplete {
        object.insert("autocomplete".into(), json!(true));
    }
    if !decl.file_types.is_empty() {
        object.insert("file_types".into(), json!(decl.file_types));
    }
    Value::Object(object)
}
