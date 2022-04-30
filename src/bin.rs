use anyhow::Result;
use pdf_explorer::file_parse_and_back;
use std::io::{self, Read};

pub fn main() -> Result<()> {
    let guard: Option<pprof::ProfilerGuard> = None;
    let mut data: Vec<u8> = vec![];
    io::stdin().read_to_end(&mut data)?;
    let out = file_parse_and_back(&data);
    println!("Called file_parse_and_back");
    std::fs::write("my-out.pdf", out.clone())?;
    assert_eq!(out, data);
    println!("Success!");
    if let Some(guard) = guard {
        let report = guard.report().build()?;
        let file = std::fs::File::create("flamegraph.svg")?;
        report.flamegraph(file)?;
    }
    Ok(())
}
