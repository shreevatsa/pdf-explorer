use pdf_explorer::file_parse_and_back;
use std::io::{self, Read};

pub fn main() {
    let mut data: Vec<u8> = vec![];
    io::stdin().read_to_end(&mut data).unwrap();
    let out = file_parse_and_back(&data);
    println!("Called file_parse_and_back");
    assert_eq!(out, data);
    println!("Success!");
}
