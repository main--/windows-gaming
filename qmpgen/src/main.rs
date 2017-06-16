extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate curl;
extern crate inflections;

mod parser;
mod types;
mod writer;

use std::env;
use std::collections::HashMap;

use curl::easy::Easy;

fn main() {
    let mut args = env::args();
    let name = args.next().unwrap();
    let (version, path) = match (args.next(), args.next()) {
        (Some(v), Some(f)) => (v,f),
        _ => {
            println!("Usage: {} [qemu-version] [export-path]", name);
            ::std::process::exit(1);
        }
    };

    let schema = download(&format!("https://raw.githubusercontent.com/qemu/qemu/{}/qapi-schema.json", version));
    let events = download(&format!("https://raw.githubusercontent.com/qemu/qemu/{}/qapi/event.json", version));

    let res = parser::parse(&schema).unwrap();
    println!("{:?}", res);
    let mut types = HashMap::new();
    types::to_types(res, &mut types);
    println!("\n\n\n");
    println!("{:?}", types);

    let res = parser::parse(&events).unwrap();
    let sections = types::to_sections(res, &mut types);
    println!("\n\n\n");
    println!("{:?}", sections);
    writer::write(path, sections, types).unwrap();
}

fn download(url: &str) -> String {
    let mut buf = Vec::new();
    {
        let mut easy = Easy::new();
        easy.url(url).unwrap();
        let mut transfer = easy.transfer();
        transfer.write_function(|data| {
            buf.extend(data);
            Ok(data.len())
        }).unwrap();
        transfer.perform().unwrap();
    }
    String::from_utf8(buf).unwrap()
}
