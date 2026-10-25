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
