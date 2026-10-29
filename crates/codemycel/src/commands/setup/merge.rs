use anyhow::{Context as _, bail};
use jsonc_parser::ast::{Object, ObjectProp, Value as Ast};
use jsonc_parser::common::Ranged as _;
use jsonc_parser::{CollectOptions, ParseOptions, parse_to_ast};
use serde_json::Value;
use toml_edit::{Array, DocumentMut, Item, Table};

/// A server entry's fields, in the order they are written.
pub type Entry = Vec<(&'static str, Value)>;

/// What `append` writes: a value, or an object whose keys keep their order.
enum Node<'e> {
    Value(&'e Value),
    Object(Vec<(&'e str, Node<'e>)>),
}

impl<'e> Node<'e> {
    fn entry(entry: &'e [(&'static str, Value)]) -> Self {
        Node::Object(
            entry
                .iter()
                .map(|(key, value)| (*key, Node::Value(value)))
                .collect(),
        )
    }
}

/// `text` (a JSON or JSONC document, or nothing) with `entry` under `servers.name`. The new
/// property is spliced in after the last one, in the layout of its neighbours, so every other
/// byte stays as it was and `json_remove` can take exactly it out again.
pub fn json_upsert(
    text: Option<&str>,
    servers: &str,
    name: &str,
    entry: &[(&'static str, Value)],
) -> anyhow::Result<String> {
    let text = text
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("{}\n");
    let root = parse_object(text)?;
    let Some((_, servers_prop)) = property(&root, servers) else {
        let wrapped = Node::Object(vec![(name, Node::entry(entry))]);
        return Ok(append(text, &root, servers, &wrapped));
    };
    let Ast::Object(map) = &servers_prop.value else {
        bail!("`{servers}` is not a JSON object");
    };
    let wanted = Value::Object(
        entry
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    );
    Ok(match property(map, name) {
        None => append(text, map, name, &Node::entry(entry)),
        Some((_, prop)) if Value::from(prop.value.clone()) == wanted => text.to_owned(),
        Some((_, prop)) => {
            let range = prop.value.range();
            let multiline = text[map.range.start..map.range.end].contains('\n');
            let indent = line_indent(text, prop.range.start);
            let value = render(&Node::entry(entry), &indent, multiline, true);
            splice(text, range.start, range.end, &value)
        }
    })
}

/// Whether `text` (a JSON or JSONC document, or nothing) holds `servers`, and `servers.name`.
pub fn json_holds(text: Option<&str>, servers: &str, name: &str) -> anyhow::Result<(bool, bool)> {
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return Ok((false, false));
    };
    let root = parse_object(text)?;
    Ok(match property(&root, servers) {
        None => (false, false),
        Some((_, prop)) => match &prop.value {
            Ast::Object(map) => (true, property(map, name).is_some()),
            _ => (true, false),
        },
    })
}

/// `text` without `servers.name`, and, when `drop_servers`, without `servers` once that leaves
/// it empty.
pub fn json_remove(
    text: &str,
    servers: &str,
    name: &str,
    drop_servers: bool,
) -> anyhow::Result<String> {
    let root = parse_object(text)?;
    let Some((servers_index, servers_prop)) = property(&root, servers) else {
        return Ok(text.to_owned());
    };
    let Ast::Object(map) = &servers_prop.value else {
        return Ok(text.to_owned());
    };
    let Some((index, _)) = property(map, name) else {
        return Ok(text.to_owned());
    };
    Ok(if drop_servers && map.properties.len() == 1 {
        cut(text, &root, servers_index)
    } else {
        cut(text, map, index)
    })
}

fn parse_object(text: &str) -> anyhow::Result<Object<'_>> {
    let parsed = parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
        .context("parsing JSON")?;
    match parsed.value {
        Some(Ast::Object(object)) => Ok(object),
        _ => bail!("the document is not a JSON object"),
    }
}

fn property<'o, 'a>(object: &'o Object<'a>, name: &str) -> Option<(usize, &'o ObjectProp<'a>)> {
    object
        .properties
        .iter()
        .enumerate()
        .find(|(_, prop)| prop.name.as_str() == name)
}

/// `object` in `text` with `"name": value` after its last property, separated from it by a
/// comma and the whitespace that precedes that property.
fn append(text: &str, object: &Object<'_>, name: &str, value: &Node<'_>) -> String {
    let key = Value::from(name);
    match object.properties.last() {
        Some(last) => {
            let start = last.range.start;
            let separator = &text[text[..start].trim_end().len()..start];
            let spaced = !separator.is_empty();
            let colon = if spaced { ": " } else { ":" };
            let multiline = separator.contains('\n');
            let value = render(value, &line_indent(text, start), multiline, spaced);
            let insert = format!(",{separator}{key}{colon}{value}");
            splice(text, last.range.end, last.range.end, &insert)
        }
        None => {
            let indent = line_indent(text, object.range.start);
            let inner = format!("{indent}  ");
            let value = render(value, &inner, true, true);
            let replacement = format!("{{\n{inner}{key}: {value}\n{indent}}}");
            splice(text, object.range.start, object.range.end, &replacement)
        }
    }
}

/// `object` in `text` without its property at `index`, along with the comma and whitespace
/// that `append` put before it.
fn cut(text: &str, object: &Object<'_>, index: usize) -> String {
    let properties = &object.properties;
    let prop = &properties[index];
    if let Some(previous) = index.checked_sub(1).map(|i| &properties[i]) {
        splice(text, previous.range.end, prop.range.end, "")
    } else if let Some(next) = properties.get(1) {
        splice(text, prop.range.start, next.range.start, "")
    } else {
        splice(text, object.range.start, object.range.end, "{}")
    }
}

/// `node` as JSON: objects one property per line under `indent` when `multiline`, arrays always
/// on one line; `spaced` puts a space after colons and commas on one line.
fn render(node: &Node<'_>, indent: &str, multiline: bool, spaced: bool) -> String {
    let (colon, comma) = if spaced { (": ", ", ") } else { (":", ",") };
    match node {
        Node::Value(Value::Array(items)) => {
            let items: Vec<String> = items
                .iter()
                .map(|item| render(&Node::Value(item), indent, false, spaced))
                .collect();
            format!("[{}]", items.join(comma))
        }
        Node::Value(value) => value.to_string(),
        Node::Object(fields) if multiline => {
            let inner = format!("{indent}  ");
            let lines: Vec<String> = fields
                .iter()
                .map(|(key, node)| {
                    let value = render(node, &inner, true, true);
                    format!("{inner}{}: {value}", Value::from(*key))
                })
                .collect();
            format!("{{\n{}\n{indent}}}", lines.join(",\n"))
        }
        Node::Object(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(key, node)| {
                    let value = render(node, indent, false, spaced);
                    format!("{}{colon}{value}", Value::from(*key))
                })
                .collect();
            format!("{{{}}}", fields.join(comma))
        }
    }
}

/// The whitespace that starts the line holding byte `at`.
fn line_indent(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..at]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn splice(text: &str, start: usize, end: usize, insert: &str) -> String {
    format!("{}{insert}{}", &text[..start], &text[end..])
}

/// `text` (a TOML document, or nothing) with the table `[servers.name]` holding `entry`, other
/// tables and comments untouched.
pub fn toml_upsert(
    text: Option<&str>,
    servers: &str,
    name: &str,
    entry: &[(&'static str, Value)],
) -> anyhow::Result<String> {
    let mut document = parse_toml(text.unwrap_or(""))?;
    let map = document.entry(servers).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    let Some(map) = map.as_table_mut() else {
        bail!("`{servers}` is not a TOML table");
    };
    let wanted = toml_table(entry)?;
    let current = map.get(name).and_then(Item::as_table);
    if current.is_none_or(|current| !same_table(current, &wanted)) {
        map.insert(name, Item::Table(wanted));
    }
    Ok(document.to_string())
}

/// Whether `text` (a TOML document, or nothing) holds the table `servers`, and `[servers.name]`.
pub fn toml_holds(text: Option<&str>, servers: &str, name: &str) -> anyhow::Result<(bool, bool)> {
    let document = parse_toml(text.unwrap_or(""))?;
    Ok(match document.get(servers) {
        None => (false, false),
        Some(map) => (true, map.get(name).is_some()),
    })
}

/// `text` without `[servers.name]`, and without `servers` when that leaves it empty and it has
/// no header of its own.
pub fn toml_remove(text: &str, servers: &str, name: &str) -> anyhow::Result<String> {
    let mut document = parse_toml(text)?;
    let Some(map) = document.get_mut(servers).and_then(Item::as_table_mut) else {
        return Ok(text.to_owned());
    };
    if map.remove(name).is_none() {
        return Ok(text.to_owned());
    }
    if map.is_empty() && map.is_implicit() {
        document.remove(servers);
    }
    Ok(document.to_string())
}

fn parse_toml(text: &str) -> anyhow::Result<DocumentMut> {
    text.parse().context("parsing TOML")
}

fn toml_table(entry: &[(&'static str, Value)]) -> anyhow::Result<Table> {
    let mut table = Table::new();
    for (key, value) in entry {
        table.insert(key, Item::Value(toml_value(value)?));
    }
    Ok(table)
}

fn toml_value(value: &Value) -> anyhow::Result<toml_edit::Value> {
    Ok(match value {
        Value::Bool(value) => (*value).into(),
        Value::String(value) => value.as_str().into(),
        Value::Array(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(toml_value(value)?);
            }
            array.into()
        }
        other => bail!("no TOML form for {other}"),
    })
}

fn same_table(current: &Table, wanted: &Table) -> bool {
    current.len() == wanted.len()
        && wanted.iter().all(|(key, value)| {
            current.get(key).and_then(Item::as_value).map(plain) == value.as_value().map(plain)
        })
}

/// A value without its formatting, for comparison.
fn plain(value: &toml_edit::Value) -> String {
    let mut value = value.clone();
    value.decor_mut().clear();
    if let Some(array) = value.as_array_mut() {
        array.fmt();
    }
    value.to_string()
}
