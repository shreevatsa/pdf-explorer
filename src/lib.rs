use js_sys::Uint8Array;
use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_till},
    character::{
        complete::{digit0, digit1, one_of},
        is_oct_digit,
    },
    combinator::opt,
    sequence::tuple,
    IResult,
};
use std::fmt;
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

// =====================
// 7.3.2 Boolean Objects
// =====================
#[derive(PartialEq, Debug)]
enum BooleanObject {
    True,
    False,
}
impl fmt::Display for BooleanObject {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BooleanObject::True => "true",
                BooleanObject::False => "false",
            }
        )
    }
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

// =====================
// 7.3.3 Numeric Objects
// =====================
// Store the sign separately, to be able to put it back.
enum Sign {
    Plus,
    Minus,
    None,
}
impl fmt::Display for Sign {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Sign::Plus => "+",
                Sign::Minus => "-",
                Sign::None => "",
            }
        )
    }
}
fn parse_sign(input: &str) -> IResult<&str, Sign> {
    let (input, sign) = opt(one_of("+-"))(input)?;
    let sign = match sign {
        None => Sign::None,
        Some('+') => Sign::Plus,
        Some('-') => Sign::Minus,
        Some(_) => unreachable!(),
    };
    Ok((input, sign))
}
// Store the digits rather than just an i64, to be able to round-trip leading 0s.
struct Integer<'a> {
    sign: Sign,
    digits: &'a str,
}
impl fmt::Display for Integer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.sign, self.digits)
    }
}
fn object_numeric_integer(input: &str) -> IResult<&str, Integer> {
    let (input, (sign, digits)) = tuple((parse_sign, digit1))(input)?;
    // let value = i64::from_str_radix(digits, 10).unwrap();
    Ok((input, Integer { sign, digits }))
}

struct Real<'a> {
    sign: Sign,
    digits_before: &'a str,
    digits_after: &'a str,
}
impl fmt::Display for Real<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}{}.{}",
            self.sign, self.digits_before, self.digits_after
        )
    }
}
fn object_numeric_real(input: &str) -> IResult<&str, Real> {
    let (input, (sign, digits_before, _, digits_after)) = tuple((
        parse_sign,
        digit0,
        nom::character::complete::char('.'),
        digit0,
    ))(input)?;
    Ok((
        input,
        Real {
            sign,
            digits_before,
            digits_after,
        },
    ))
}
enum NumericObject<'a> {
    Integer(Integer<'a>),
    Real(Real<'a>),
}
impl fmt::Display for NumericObject<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NumericObject::Integer(i) => write!(f, "{}", i),
            NumericObject::Real(r) => write!(f, "{}", r),
        }
    }
}

fn object_numeric(input: &str) -> IResult<&str, NumericObject> {
    let real = object_numeric_real(input);
    match real {
        Ok((input, real)) => Ok((input, NumericObject::Real(real))),
        Err(_) => {
            let (input, integer) = object_numeric_integer(input)?;
            Ok((input, NumericObject::Integer(integer)))
        }
    }
}

// =====================
// 7.3.4 String Objects
// =====================

// 7.3.4.2 Literal Strings
// Sequence of bytes, where only \ has special meaning. (Note: When encoding, also need to escape unbalanced parentheses.)
// To be able to round-trip successfully, we'll store it as alternating sequences of <part before \, the special part after \>
struct LiteralString<'a> {
    parts: Vec<(&'a str, Option<&'a str>)>,
}
impl fmt::Display for LiteralString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (before, after) in &self.parts {
            write!(f, "{}", before).unwrap();
            match after {
                Some(v) => write!(f, "\\{}", v),
                None => write!(f, "{}", ""),
            }
            .unwrap();
        }
        write!(f, "{}", "")
    }
}

fn object_literal_string<'a>(input: &'a str) -> IResult<&str, LiteralString> {
    let mut parts: Vec<(&'a str, Option<&'a str>)> = vec![];
    let mut input = input;
    let mut result;
    loop {
        // The 'before' part
        (input, result) = take_till(|c| c == '\\')(input)?;
        if input == "" {
            parts.push((result, None));
            break;
        }
        let first = input.bytes().nth(0).unwrap() as char;
        if "nrtbf()\\".contains(first) {
            parts.push((result, Some(&input[..=0])));
            input = &input[1..];
            continue;
        }
        // Two more cases: three octal digits, or end-of-line marker
        let whatever = eol_marker(input);
        if whatever.is_ok() {
            let eol;
            (input, eol) = whatever.unwrap();
            parts.push((result, Some(eol)));
        } else {
            // Up to three octal digits
            let mut oct = &input[..3];
            assert!(is_oct_digit(oct.bytes().nth(0).unwrap()));
            if is_oct_digit(oct.bytes().nth(1).unwrap()) {
                if is_oct_digit(oct.bytes().nth(2).unwrap()) {
                    input = &input[3..];
                } else {
                    oct = &input[..2];
                    input = &input[2..];
                }
            } else {
                oct = &input[..1];
                input = &input[1..];
            }
            parts.push((result, Some(oct)));
        }
    }
    Ok((input, LiteralString { parts }))
}

fn eol_marker(input: &str) -> IResult<&str, &str> {
    alt((tag("\r\n"), tag("\r"), tag("\n")))(input)
}

// ===========
// 7.3 Objects
// ===========
enum Object<'a> {
    Boolean(BooleanObject),
    Numeric(NumericObject<'a>),
}
impl fmt::Display for Object<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Object::Boolean(o) => write!(f, "{}", o),
            Object::Numeric(o) => write!(f, "{}", o),
        }
    }
}

fn object(input: &str) -> IResult<&str, Object> {
    let try_boolean = object_boolean(input);
    let (input, object) = match try_boolean {
        Ok((input, result)) => (input, Object::Boolean(result)),
        Err(_) => {
            let (input, result) = object_numeric(input)?;
            (input, Object::Numeric(result))
        }
    };
    Ok((input, object))
}

#[test]
fn round_trip() {
    for input in [
        "true", "false", //
        "123", "43445", "+17", "-98", "0", //
        "0042", "-0042", //
        "34.5", "-3.62", "+123.6", "4.", "-.002", "0.0", //
    ] {
        let (remaining, result) = object(input).unwrap();
        assert_eq!(remaining, "");
        let out = result.to_string();
        println!("{} vs {}", input, out);
        assert_eq!(input, out);
    }
}
