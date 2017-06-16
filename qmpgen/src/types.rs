use std::collections::{HashMap, VecDeque};

use serde_json::Value;

use parser::{
    Part,
    Description,
    Doc,
    Object,
    Enum,
    Struct,
    Union,
    Event,
    Rest,
};

#[derive(Clone, Debug)]
pub struct Documentation {
    pub name: String,
    pub documentation: String,
    pub example: String,
    pub parameters: Vec<(String, String)>,
    pub since: String,
    pub notes: Vec<String>,
    pub returns: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub doc: Documentation,
    pub typ: Type,
}

#[derive(Clone, Debug)]
pub enum Type {
    Empty,
    String,
    F64,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    Bool,
    Existing(String),
    Enum(Vec<String>),
    Union((Option<String>, Vec<(String, Type)>)),
    List(Box<Type>),
    Map(Vec<(String, Type)>)
}

fn parse_desc(desc: Description) -> Documentation {
    let Description { name, documentation, example, rest } = desc;
    let mut params = Vec::new();
    let mut since = None;
    let mut notes = Vec::new();
    let mut returns = None;
    for rest in rest {
        match rest {
            Rest::Parameter((name, doc)) => params.push((name, doc)),
            Rest::Since(s) => since = Some(s),
            Rest::Note(note) => notes.push(note),
            Rest::Returns(ret) => returns = Some(ret),
        }
    }
    Documentation {
        name: name,
        documentation: documentation,
        example: example,
        parameters: params,
        since: since.expect("since"),
        notes: notes,
        returns: returns,
    }
}

pub fn to_sections(parts: Vec<Part>, types: &mut HashMap<String, Type>) -> Vec<Section> {
    let mut res = Vec::new();

    to_types(parts.clone(), types);

    for Part { description: desc, object } in parts {
        let doc = match desc {
            Doc::Parsed(desc) => parse_desc(desc),
            Doc::Unparsed(s) => panic!("Got unparsed doc: {}", s)
        };

        match object_to_type(object, &types) {
            Ok(Some((name, typ))) => { println!("Converted {}", name); res.push(Section { name, doc, typ }); },
            Err(object) => println!("Couldn't convert {:?}", object),
            _ => ()
        }
    }
    res
}

pub fn to_types(parts: Vec<Part>, types: &mut HashMap<String, Type>) {
    types.insert("str".to_string(), Type::String);
    types.insert("number".to_string(), Type::F64);
    types.insert("int".to_string(), Type::I64);
    types.insert("int8".to_string(), Type::I8);
    types.insert("int16".to_string(), Type::I16);
    types.insert("int32".to_string(), Type::I32);
    types.insert("int64".to_string(), Type::I64);
    types.insert("uint8".to_string(), Type::U8);
    types.insert("uint16".to_string(), Type::U16);
    types.insert("uint32".to_string(), Type::U32);
    types.insert("uint64".to_string(), Type::U64);
    types.insert("size".to_string(), Type::U64);
    types.insert("bool".to_string(), Type::Bool);
    types.insert("any".to_string(), Type::String);
    types.insert("QType".to_string(), Type::String);

    let mut todo = VecDeque::new();

    for Part { description: _, object } in parts {
        match object_to_type(object, &types) {
            Ok(Some((name, typ))) => { println!("Add {}", name); types.insert(name, typ); },
            Err(object) => todo.push_back(object),
            _ => ()
        }
    }

    let mut len = 0;
    while len != todo.len() {
        len = todo.len();
        for _ in 0..todo.len() {
            let object = todo.pop_front().unwrap();
            match object_to_type(object, &types) {
                Ok(Some((name, typ))) => {
                    println!("Add late {}", name);
                    types.insert(name, typ);
                },
                Err(object) => todo.push_back(object),
                _ => ()
            }
        }
    }
    println!("rest: {:?}", todo);
}

fn object_to_type(object: Object, types: &HashMap<String, Type>) -> Result<Option<(String, Type)>, Object> {
    match object {
        Object::Enum(Enum { name, data, gen }) => {
            Ok(Some((name, Type::Enum(data))))
        },
        Object::Struct(s) => {
            let backup = s.clone();
            let Struct { name, data, base, gen } = s;
            let mut map = if let Some(base) = base {
                if let Type::Map(ref map) = types[&base] {
                    map.clone()
                } else {
                    panic!("Got base class for {} which is not a map: {}", name, base);
                }
            } else {
                Vec::new()
            };
            if let Some(val) = data {
                let typ = value_to_type(val, types);
                match typ {
                    Ok(res) => {
                        if let Type::Map(m) = res {
                            map.extend(m);
                        } else {
                            panic!("Got data which is not a map for struct {}: {:?}", name, res)
                        }
                    },
                    Err(val) => {
                        return Err(Object::Struct(backup));
                    }
                }
            }
            Ok(Some((name, Type::Map(map))))
        },
        Object::Union(u) => {
            let backup = u.clone();
            let Union { union, base, discriminator, data, gen } = u;
            if base.is_some() || discriminator.is_some() {
                // I don't know how to parse them here
                println!("Ignoring union {}", union);
                return Ok(None);
            }
            let mut vec = Vec::new();
             for (k, v) in data.into_iter().flat_map(|m| m.into_iter()) {
                 let typ = match types.get(&v).cloned() {
                     Some(t) => t,
                     None => return Err(Object::Union(backup))
                 };
                 vec.push((k, typ));
             }
            Ok(Some((union, Type::Union((discriminator, vec)))))
        },
        Object::Event(evt) => {
            let backup = evt.clone();
            let Event { event, data } = evt;
            if let Some(val) = data {
                match value_to_type(val, types) {
                    Ok(t) => Ok(Some((event, t))),
                    _ => Err(Object::Event(backup))
                }
            } else {
                Ok(Some((event, Type::Empty)))
            }
        }
        _ => Ok(None)
    }
}

fn value_to_type(val: Value, types: &HashMap<String, Type>) -> Result<Type, Value> {
    match val {
        Value::String(s) => {
            let res = string_to_type(s.clone(), types).map_err(|_| Value::String(s));
            if let Err(Value::String(ref s)) = res {
                println!("Missing {}", s);
            }
            res
        },
        Value::Array(mut vec) => {
            assert!(vec.len() == 1);
            if let Value::String(s) = vec.remove(0) {
                let res = string_to_type(s.clone(), types).map(|t| Type::List(Box::new(t))).map_err(|_| Value::Array(vec![Value::String(s)]));
                if let Err(Value::Array(ref a)) = res {
                    if let Value::String(ref s) = a[0] {
                        println!("Missing {}", s);
                    }
                }
                res
            } else {
                unreachable!()
            }
        },
        Value::Object(map) => {
            let backup = map.clone();
            let mut res = Vec::new();
            for (k, v) in map {
                let v = match value_to_type(v, types) {
                    Ok(v) => v,
                    Err(val) => return Err(Value::Object(backup))
                };
                res.push((k, v));
            }
            Ok(Type::Map(res))
        },
        _ => unreachable!()
    }
}

fn string_to_type(s: String, types: &HashMap<String, Type>) -> Result<Type, ()> {
    types.get(&s).map(|t| match *t {
        Type::Union(_) | Type::Enum(_) | Type::List(_) | Type::Map(_) => Type::Existing(s),
        ref t => t.clone()
    }).ok_or(())
}
