use std::io::{self, Write, BufRead, StdinLock};
use std::path::Path;
use std::ops::Range;
use std::str::FromStr;
use std::fmt::Display;

pub fn yesno(stdin: &mut StdinLock, question: &str) -> bool {
    anything(stdin, question, "y/n", |line| {
        if line == "y" { Some(true) }
        else if line == "n" { Some(false) }
        else { None }
    })
}

pub fn numeric<T : PartialOrd<T> + FromStr + Copy + Display>(stdin: &mut StdinLock, question: &str, range: Range<T>) -> T {
    // bug: the options string is off by one as the end index is exclusive
    anything(stdin, question, &format!("{}..{}", range.start, range.end), |line| {
        line.parse().ok().and_then(|x| if (range.start <= x) && (range.end > x) { Some(x) } else { None })
    })
}

pub fn file(stdin: &mut StdinLock, question: &str) -> String {
    anything(stdin, question, "", |x| {
        if Path::new(x).exists() {
            Some(x.to_owned())
        } else {
            None
        }
    })
}

pub fn anything<T, F: Fn(&str) -> Option<T>>(stdin: &mut StdinLock, question: &str, options: &str, parse_answer: F) -> T {
    let mut line = String::new();
    loop {
        print!("{} [{}] ", question, options);
        io::stdout().flush().ok().expect("Could not flush stdout");
        stdin.read_line(&mut line).unwrap();

        if let Some(t) = parse_answer(&line[..(line.len() - 1)]) {
            return t;
        }

        line.clear();
    }
}
