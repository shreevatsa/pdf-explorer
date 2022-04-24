use js_sys::Uint8Array;
use nom::{branch::alt, bytes::complete::tag, IResult};
use wasm_bindgen::prelude::*;
use web_sys::{console, File, FileReaderSync};

// Consider a file containing three "characters": mकn
// • m U+006D LATIN SMALL LETTER M
// • क U+0915 DEVANAGARI LETTER KA
// • n U+006E LATIN SMALL LETTER N
// It is saved in UTF-8 as five bytes: 6d e0 a4 95 6e
// But `readAsBinaryString` reads into a DOMString,
// which is "a sequence of 16-bit unsigned integers":
// in this case, a sequence of *FIVE* 16-bit integers.
// These 5 integers, interpreted as USVs, would be:
// • m U+006D LATIN SMALL LETTER M
// • à U+00E0 LATIN SMALL LETTER A WITH GRAVE
// • ¤ U+00A4 CURRENCY SIGN
// • � U+0095 MESSAGE WAITING
// • n U+006E LATIN SMALL LETTER N
// Unfortunately, that sequence of "characters" is what gets encoded into UTF-8,
// and the resulting 8 bytes are the (Rust) String we get from there.
// But we can instead use `read_as_array_buffer`! See below.
#[wasm_bindgen]
pub fn handle_file(file: File) -> u32 {
    console::log_1(&format!("in Rust handle_file").into());
    let filereader = FileReaderSync::new().unwrap();
    let buffer = filereader.read_as_array_buffer(&file).unwrap();
    let view = Uint8Array::new(&buffer); // This is instant.
    console::log_1(&format!("read {} bytes to ArrayBuffer", view.byte_length()).into());
    let v = view.to_vec();
    console::log_1(&format!("copied into Vec<u8>, computing crc32").into());
    let crc32 = crc32fast::hash(&v);
    console::log_1(&format!("done: crc32 {:x}", crc32).into());
    crc32
}

#[derive(PartialEq, Debug)]
enum BooleanObject {
    True,
    False,
}

fn object_boolean(input: &str) -> IResult<&str, BooleanObject> {
    let (input, res) = alt((tag("true"), tag("false")))(input)?;
    let ret = if res == "true" {
        BooleanObject::True
    } else if res == "false" {
        BooleanObject::False
    } else {
        unreachable!();
    };
    Ok((input, ret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_boolean_true() {
        let (rest, result) = object_boolean("trueasdf").unwrap();
        assert_eq!(rest, "asdf");
        assert_eq!(result, BooleanObject::True);
    }

    #[test]
    fn parse_boolean_false() {
        let (rest, result) = object_boolean("falseasdf").unwrap();
        assert_eq!(rest, "asdf");
        assert_eq!(result, BooleanObject::False);
    }

    #[test]
    fn parse_boolean_none() {
        let err = object_boolean("asdf");
        assert!(err.is_err());
    }
}
