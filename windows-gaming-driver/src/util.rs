pub fn parse_hex(s: &str) -> Option<u16> {
    if s.starts_with("0x") {
        u16::from_str_radix(&s[2..], 16).ok()
    } else {
        u16::from_str_radix(s, 16).ok()
    }
}
