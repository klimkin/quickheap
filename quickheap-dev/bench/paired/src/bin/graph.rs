#[path = "../graph.rs"]
mod graph;

use std::io::{self, Write};
use std::path::PathBuf;

fn run() -> Result<(), String> {
    let mut path = None;
    let mut workload = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                println!(
                    "Usage: graph FILE.gr [--workload Dijkstra|Prim] [--verify-only]\nSource vertex: 0. Prim uses minimum-edge undirected conversion.\nDijkstra rejects candidate distances >= UINT32_MAX."
                );
                return Ok(());
            }
            "--verify-only" => {}
            "--workload" => {
                workload = Some(arguments.next().ok_or("missing value for --workload")?);
            }
            value if value.starts_with('-') => return Err(format!("unknown option {value}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("expected one graph path".into());
                }
            }
        }
    }
    let path = path.ok_or("expected graph path")?;
    let trace = graph::verify_file(&path, workload.as_deref())?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    output
        .write_all(trace.as_bytes())
        .map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("quickheap_paired_graph: {error}");
        std::process::exit(1);
    }
}
