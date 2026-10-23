use serde_json::{Map, Value};
use toml_edit::{Item, Table};

use crate::Result;

pub(super) fn decode(item: &Item) -> Result<Value> {
    match item {
        Item::None => Ok(Value::Null),
        Item::Value(value) => decode_value(value),
        Item::Table(table) => table
            .iter()
            .map(|(key, value)| Ok((key.to_owned(), decode(value)?)))
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        Item::ArrayOfTables(tables) => tables
            .iter()
            .map(|table| decode(&Item::Table(table.clone())))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
    }
}

fn decode_value(value: &toml_edit::Value) -> Result<Value> {
    match value {
        toml_edit::Value::String(value) => Ok(Value::String(value.value().clone())),
        toml_edit::Value::Integer(value) => Ok((*value.value()).into()),
        toml_edit::Value::Boolean(value) => Ok((*value.value()).into()),
        toml_edit::Value::Array(array) => array
            .iter()
            .map(decode_value)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        toml_edit::Value::InlineTable(table) => table
            .iter()
            .map(|(key, value)| Ok((key.to_owned(), decode_value(value)?)))
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        _ => Err(super::invalid(
            "registry accepts no floating point or datetime values",
        )),
    }
}

pub(super) fn encode(value: Value) -> Result<Item> {
    match value {
        Value::Object(map) => {
            let mut table = Table::new();
            for (key, value) in map {
                if !value.is_null() {
                    table.insert(&key, encode(value)?);
                }
            }
            Ok(Item::Table(table))
        }
        value => Ok(Item::Value(encode_value(value)?)),
    }
}

fn encode_value(value: Value) -> Result<toml_edit::Value> {
    Ok(match value {
        Value::String(value) => value.into(),
        Value::Bool(value) => value.into(),
        Value::Number(value) => value
            .as_i64()
            .ok_or_else(|| super::invalid("invalid registry number"))?
            .into(),
        Value::Array(array) => {
            let mut encoded = toml_edit::Array::new();
            for value in array {
                encoded.push(encode_value(value)?);
            }
            encoded.into()
        }
        _ => return Err(super::invalid("unsupported registry value")),
    })
}
