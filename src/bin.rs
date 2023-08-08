// @<bin
use anyhow::Result;
use pdf_explorer::file_parse_and_back;
use std::io::{self, Read};

/// This is a simple binary wrapper around `file_parse_and_back`:
/// - Reads a PDF file from stdin,
/// - Calls `file_parse_and_back` on it,
/// - (Saves to a file and) compares the bytes of the input and output PDF files.
pub fn main() -> Result<()> {
    let mut data: Vec<u8> = vec![];
    io::stdin().read_to_end(&mut data)?;
    let out: Vec<u8> = file_parse_and_back(&data);
    println!("Called file_parse_and_back");
    std::fs::write("parsed-file-written-back.pdf", out.clone())?;
    if out != data {
        println!(
            "Unequal serializations: length {} vs length {}",
            out.len(),
            data.len()
        );
        /*
        let i = out
            .iter()
            .zip(data.iter())
            .take_while(|&(o, d)| o == d)
            .count();
        */
        let mut i = 0;
        while i < out.len() && i < data.len() && out[i] == data[i] {
            i += 1;
        }
        println!("The first {} bytes are equal.", i);
        let start = std::cmp::max(0, i - 10);
        assert_eq!(&out[start..], &data[start..]);
    }
    assert_eq!(out, data);
    println!("Success!");
    Ok(())
}
// >@bin
