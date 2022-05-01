use anyhow::Result;
use pdf_explorer::file_parse_and_back;
use std::io::{self, Read};

pub fn main() -> Result<()> {
    let mut data: Vec<u8> = vec![];
    io::stdin().read_to_end(&mut data)?;
    let out = file_parse_and_back(&data);
    println!("Called file_parse_and_back");
    std::fs::write("my-out.pdf", out.clone())?;
    if out != data {
        println!(
            "Unequal serializations: length {} vs length {}",
            out.len(),
            data.len()
        );
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
