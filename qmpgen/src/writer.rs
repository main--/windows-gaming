use std::io::{Write, Result};
use std::path::Path;
use std::fs::File;
use std::iter;
use std::collections::HashMap;

use types::{Section, Documentation, Type};

macro_rules! w {
    ($w:ident) => { writeln!($w) };
    ($w:ident, $indent:ident, $s:expr) => { writeln!($w, "{}{}", indent($indent), $s) };
    ($w:ident, $indent:ident, $fmt:expr, $($arg:tt),+) => { writeln!($w, "{}{}", indent($indent), format!($fmt, $($arg),+)) };
}

pub fn write<P: AsRef<Path>>(path: P, sections: Vec<Section>, mut types: HashMap<String, Type>) -> Result<()> {
    let mut file = File::create(path).expect("Unknown path");
    write_internal(&mut file, sections, &mut types, 0)
}

fn write_internal<W: Write>(w: &mut W, sections: Vec<Section>, types: &mut HashMap<String, Type>, i: usize) -> Result<()> {
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
        println!("{:?}", ex);
        let ex = ex.replace("\n##", "").replace("\n#", "\n")[1..].trim().to_string();
        println!("{:?}", ex);
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
        write_type(w, name, typ, i)?;
        w!(w)?;
    }
    Ok(())
}

fn write_type<W: Write>(w: &mut W, name: String, typ: Type, indent: usize) -> Result<()> {
    Ok(())
}

fn indent(indent: usize) -> String {
    iter::repeat(" ").take(indent).collect()
}
