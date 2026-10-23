#[path = "../src/fixture.rs"]
mod fixture;

use fixture::{INSPECT_HEADER, InputFixture};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: fixture [--inspect] PATH [--dump]")?;
    let path = if first == "inspect" || first == "--inspect" {
        args.next()
            .ok_or("usage: fixture --inspect PATH [--dump]")?
    } else {
        first
    };
    let dump = match args.next().as_deref() {
        None => false,
        Some("--dump") => true,
        Some(_) => return Err("usage: fixture [--inspect] PATH [--dump]".into()),
    };
    if args.next().is_some() {
        return Err("usage: fixture [--inspect] PATH [--dump]".into());
    }
    let fixture = InputFixture::read(path)?;
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    writeln!(out, "{INSPECT_HEADER}")?;
    writeln!(out, "{}", fixture.inspect_row())?;
    if dump {
        for (label, values) in [("setup", &fixture.setup), ("timed", &fixture.timed)] {
            write!(out, "{label}")?;
            for value in values {
                write!(out, ",{value}")?;
            }
            writeln!(out)?;
        }
    }
    out.flush()?;
    Ok(())
}
