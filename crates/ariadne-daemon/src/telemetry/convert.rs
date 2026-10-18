//! Turning OpenTelemetry's own value types into the JSON the telemetry
//! store holds attributes and labels as.

use opentelemetry::logs::AnyValue;
use opentelemetry::{Key, KeyValue, Value};
use serde_json::{Map, Value as Json};

/// An OTel log attribute value, as JSON.
pub(super) fn any_value_to_json(value: &AnyValue) -> Json {
    match value {
        AnyValue::Int(i) => Json::from(*i),
        AnyValue::Double(d) => serde_json::Number::from_f64(*d)
            .map(Json::Number)
            .unwrap_or(Json::Null),
        AnyValue::String(s) => Json::String(s.as_ref().to_string()),
        AnyValue::Boolean(b) => Json::Bool(*b),
        AnyValue::Bytes(bytes) => Json::String(hex::encode(bytes.as_slice())),
        AnyValue::ListAny(list) => Json::Array(list.iter().map(any_value_to_json).collect()),
        AnyValue::Map(map) => Json::Object(
            map.iter()
                .map(|(k, v)| (k.as_str().to_string(), any_value_to_json(v)))
                .collect(),
        ),
        // `AnyValue` is `#[non_exhaustive]`; a later SDK variant this build
        // does not know is stored as null rather than refusing the record.
        _ => Json::Null,
    }
}

/// The body of a log record: its text if it is one, else its JSON form —
/// `tracing`'s own events always give a string (the formatted `message`
/// field), so this is the fallback for a producer that does not.
pub(super) fn any_value_to_body(value: &AnyValue) -> String {
    match value {
        AnyValue::String(s) => s.as_ref().to_string(),
        other => any_value_to_json(other).to_string(),
    }
}

/// A log record's attributes, as a JSON object.
pub(super) fn log_attributes_to_json<'a>(
    attributes: impl Iterator<Item = &'a (Key, AnyValue)>,
) -> Json {
    let mut object = Map::new();
    for (key, value) in attributes {
        object.insert(key.as_str().to_string(), any_value_to_json(value));
    }
    Json::Object(object)
}

/// A metric attribute value, as JSON — always a string, since every label
/// the telemetry store keys a series under is PromQL label text.
fn metric_value_to_json(value: &Value) -> Json {
    Json::String(value.as_str().into_owned())
}

/// A metric point's labels, as a JSON object.
pub(super) fn metric_labels_to_json<'a>(attributes: impl Iterator<Item = &'a KeyValue>) -> Json {
    let mut object = Map::new();
    for kv in attributes {
        object.insert(kv.key.as_str().to_string(), metric_value_to_json(&kv.value));
    }
    Json::Object(object)
}
