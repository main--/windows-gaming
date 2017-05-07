use std::io::{BufRead, BufReader, Result as IoResult};
use std::fs::File;

pub fn hwid_resolve_usb(vendor_id: u16, product_id: u16) -> IoResult<Option<(String, Option<String>)>> {
    let mut vendor = None;
    for line in BufReader::new(File::open("/usr/share/hwdata/usb.ids")?).lines() {
        let line = line?;

        if line.starts_with("#") {
            continue;
        } else if vendor.is_some() && line.starts_with("\t") && line.len() > 5 {
            if u16::from_str_radix(&line[1..5], 16) == Ok(product_id) {
                return Ok(Some((vendor.unwrap(), Some(line[5..].trim().to_owned()))));
            }
        } else if vendor.is_some() {
            // vendor is over, product not found
            return Ok(Some((vendor.unwrap(), None)));
        } else if line.len() > 4 {
            if u16::from_str_radix(&line[..4], 16) == Ok(vendor_id) {
                vendor = Some(line[4..].trim().to_owned());
            }
        }
    }
    Ok(None)
}
