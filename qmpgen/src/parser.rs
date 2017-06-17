use std::collections::HashMap;
use std::fmt;

use serde_json;
use serde_json::value::Value;

#[derive(Debug, Clone)]
pub struct Part {
    pub description: Doc,
    pub object: Object,
}

#[derive(Debug, Clone)]
pub enum Doc {
    Parsed(Description),
    Unparsed(String),
}

#[derive(Debug, Clone)]
pub struct Description {
    pub name: String,
    pub documentation: String,
    pub example: String,
    pub rest: Vec<Rest>,
}

#[derive(Debug, Clone)]
pub enum Rest {
    Parameter((String, String)),
    Since(String),
    Note(String),
    Returns(String),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Object {
    Pragma(Pragma),
    Include(Include),
    Command(Command),
    Event(Event),
    Enum(Enum),
    Struct(Struct),
    Union(Union),
}

#[derive(Deserialize, Debug, Clone)]
pub struct Pragma {
    pub pragma: Value,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Include {
    pub include: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Command {
    pub command: String,
    pub data: Option<Value>,
    pub returns: Option<Value>,
    pub gen: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Event {
    pub event: String,
    pub data: Option<Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Enum {
    #[serde(rename = "enum")] pub name: String,
    pub data: Vec<String>,
    pub gen: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Struct {
    #[serde(rename = "struct")] pub name: String,
    pub data: Option<Value>,
    pub base: Option<String>,
    pub gen: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Union {
    pub union: String,
    pub base: Option<Value>,
    pub discriminator: Option<String>,
    pub data: Option<HashMap<String, String>>,
    pub gen: Option<bool>,
}

include!(concat!(env!("OUT_DIR"), "/qapi.rs"));
