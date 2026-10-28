use serde_json::{Map, Value, json};

use crate::query::answer::Answer;

/// Renders an answer as one JSON object: `outcome`, one array per section keyed by its name,
/// `truncated` (or null), and `freshness`.
pub struct JsonFormatter;

impl JsonFormatter {
    pub fn render(answer: &Answer) -> Value {
        let mut object = Map::new();
        object.insert("outcome".to_owned(), json!(answer.outcome));
        for section in &answer.sections {
            object.insert(section.name.clone(), json!(section.lines));
        }
        object.insert("truncated".to_owned(), json!(answer.truncated));
        object.insert("freshness".to_owned(), json!(answer.freshness));
        Value::Object(object)
    }
}
