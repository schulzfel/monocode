//! JSON Schemas for operation inputs and outputs, generated the way MCP
//! expects them: draft 2020-12, root type `object`, and no top-level title or
//! description (the wrapper type's name and doc are noise to a model).
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::{Map, Value};

/// The schema of `T` as a JSON object.
pub(crate) fn object_schema<T: JsonSchema>() -> Map<String, Value> {
    let schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>();
    let mut object = match serde_json::to_value(schema) {
        Ok(Value::Object(object)) => object,
        _ => Map::new(),
    };
    object.remove("title");
    object.remove("description");
    // Some clients require `properties` even when a tool takes no arguments.
    if object.get("type") == Some(&Value::from("object")) {
        object
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
    }
    object
}

/// Publishes an enum tagged on `type` as one object instead of its derived
/// `oneOf`: MCP wants an object at the root of a tool's input, and some
/// clients refuse `oneOf` there. The tag lists every variant with its
/// description, each field appears once with its first description, and
/// only the tag is required. The input still deserializes into the enum,
/// which holds each variant to its own fields.
pub(crate) fn one_object(schema: &mut schemars::Schema) {
    let Some(Value::Array(variants)) = schema.get("oneOf").cloned() else {
        return;
    };
    let mut properties = Map::new();
    let mut tags = Vec::new();
    let mut kinds = Vec::new();
    for variant in &variants {
        let Some(Value::Object(fields)) = variant.get("properties") else {
            continue;
        };
        for (name, field) in fields {
            if name == "type" {
                if let Some(tag) = field.get("const") {
                    tags.push(tag.clone());
                }
                if let Some(Value::String(about)) = variant.get("description") {
                    kinds.push(about.clone());
                }
            } else if !properties.contains_key(name) {
                properties.insert(name.clone(), field.clone());
            }
        }
    }
    let mut tag = Map::new();
    tag.insert("type".into(), "string".into());
    tag.insert("enum".into(), Value::Array(tags));
    tag.insert("description".into(), kinds.join(" ").into());
    properties.insert("type".into(), Value::Object(tag));
    let mut object = Map::new();
    object.insert("type".into(), "object".into());
    object.insert("properties".into(), Value::Object(properties));
    object.insert("required".into(), Value::Array(vec!["type".into()]));
    object.insert("additionalProperties".into(), false.into());
    *schema = schemars::Schema::from(object);
}
