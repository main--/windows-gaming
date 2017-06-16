use std::io::{Write, Result};
use std::path::Path;
use std::fs::File;
use std::iter;
use std::collections::HashMap;

use types::{Section, Documentation, Type};

macro_rules! w {
    ($w:ident) => { writeln!($w) };
    ($w:ident, $indent:expr, $s:expr) => { writeln!($w, "{}{}", indent($indent), $s) };
    ($w:ident, $indent:expr, $fmt:expr, $($arg:expr),+) => { writeln!($w, "{}{}", indent($indent), format!($fmt, $($arg),+)) };
}

pub fn write<P: AsRef<Path>>(path: P, sections: Vec<Section>, mut types: HashMap<String, Type>) -> Result<()> {
    let mut file = File::create(path).expect("Unknown path");
    write_internal(&mut file, sections, &mut types, 0)
}

fn write_internal<W: Write>(w: &mut W, sections: Vec<Section>, types: &mut HashMap<String, Type>, i: usize) -> Result<()> {
    let mut todos = Vec::new();
    for Section { name, doc, typ } in sections {
        let Documentation {
            name: doc_name,
            documentation: doc,
            example: ex,
            parameters: params,
            since: since,
            notes: notes,
            returns: returns,
        } = doc;
        assert_eq!(name, doc_name, "Name and Doc-Name must equal");

        // TODO: proper example
        let ex = ex.replace("\n##", "").replace("\n#", "\n")[1..].trim().to_string();
        let ex = ex.replace("\n", &format!("\n{}///", indent(i)));

        w!(w, i, "/// {}", doc)?;
        w!(w, i, "///")?;
        w!(w, i, "/// Since qemu version {}", since)?;
        w!(w, i, "///")?;
        for note in notes {
            w!(w, i, "/// Note: {}", note)?;
            w!(w, i, "///")?;
        }
        let write_params = params.len() > 0;
        if write_params {
            w!(w, i, "/// # Parameters")?;
            w!(w, i, "///")?;
        }
        for (name, doc) in params {
            w!(w, i, "/// * {}: {}", name, doc)?;
        }
        if write_params {
            w!(w, i, "///")?;
        }
        if let Some(returns) = returns {
            w!(w, i, "/// # Returns {}", returns)?;
            w!(w, i, "///")?;
        }
        w!(w, i, "/// # Example")?;
        w!(w, i, "///")?;
        w!(w, i, "/// ```")?;
        w!(w, i, "/// {}", ex)?;
        w!(w, i, "/// ```")?;

        todos.extend(write_complex_type(w, name, typ, i)?);
        w!(w)?;
    }
    // write all missing
    while {
        let todos_new = match todos.pop().unwrap() {
            Todo::Existing(s) => {
                match types.remove(&s) {
                    Some(typ) => write_complex_type(w, s.clone(), typ, 0)?,
                    None => { println!("Let's just assume we already wrote {}", s); vec![] }
                }
            },
            Todo::New(s, t) => write_complex_type(w, s, t, 0)?,
        };
        todos.extend(todos_new);
        todos.len() != 0
    } {}
    Ok(())
}

enum Todo {
    Existing(String),
    New(String, Type),
}

fn write_complex_type<W: Write>(w: &mut W, type_name: String, typ: Type, indent: usize) -> Result<Vec<Todo>> {
    let mut todos = Vec::new();
    match typ {
        Type::Enum(variants) => {
            w!(w, indent, "pub enum {} {{", type_name)?;
            for variant in variants {
                w!(w, indent+4, "{},", variant)?;
            }
            w!(w, indent, "}")?;
        },
        Type::Union(_, variants) => {
            w!(w, indent, "pub enum {} {{", type_name)?;
            for (name, typ) in variants {
                w!(w, indent+4, "{}({}),", name, simple_type(typ, &mut todos).unwrap())?;
            }
            w!(w, indent, "}")?;
        },
        Type::Map(map) => {
            w!(w, indent, "pub struct {} {{", type_name)?;
            for (name, typ) in map {
                if let Some(typ) = simple_type(typ.clone(), &mut todos) {
                    w!(w, indent+4, "{}: {},", name, typ)?;
                } else {
                    let new_name = type_name.clone() + "_" + &name.to_uppercase();
                    w!(w, indent+4, "{}: {},", name, new_name)?;
                    todos.push(Todo::New(new_name, typ));
                }
            }
            w!(w, indent, "}")?;
        },
        _ => unreachable!()
    }
    Ok(todos)
}

fn simple_type(typ: Type, todos: &mut Vec<Todo>) -> Option<String> {
    match typ {
        Type::Bool => Some("bool".to_string()),
        Type::F64 => Some("f64".to_string()),
        Type::I8 => Some("i8".to_string()),
        Type::I16 => Some("i16".to_string()),
        Type::I32 => Some("i32".to_string()),
        Type::I64 => Some("i64".to_string()),
        Type::U8 => Some("u8".to_string()),
        Type::U16 => Some("u16".to_string()),
        Type::U32 => Some("u32".to_string()),
        Type::U64 => Some("u64".to_string()),
        Type::String => Some("String".to_string()),
        Type::Existing(name) => {
            todos.push(Todo::Existing(name.clone()));
            Some(name)
        },
        Type::Option(typ) => Some(format!("Option<{}>", simple_type(*typ, todos).unwrap())),
        Type::List(typ) => Some(format!("Vec<{}>", simple_type(*typ, todos).unwrap())),
        _ => None
    }
}

fn indent(indent: usize) -> String {
    iter::repeat(" ").take(indent).collect()
}
