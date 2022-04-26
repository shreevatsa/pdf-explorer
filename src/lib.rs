use js_sys::Uint8Array;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while_m_n},
    character::{
        complete::{digit0, digit1, one_of},
        is_oct_digit,
    },
    combinator::{map, opt},
    sequence::tuple,
    IResult,
};
use std::{fmt, io::Write};
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

// Serializing to bytes, instead of str
pub trait Serialize {
    fn serialize(&self, buf: &mut [u8]);
}

// =====================
// 7.3.2 Boolean Objects
// =====================
#[derive(PartialEq, Debug)]
enum BooleanObject {
    True,
    False,
}
impl Serialize for BooleanObject {
    fn serialize(&self, mut buf: &mut [u8]) {
        buf.write_all(match self {
            BooleanObject::True => b"true",
            BooleanObject::False => b"false",
        })
        .unwrap();
    }
}
fn object_boolean(input: &[u8]) -> IResult<&[u8], BooleanObject> {
    let (input, result) = alt((tag("true"), tag("false")))(input)?;
    let ret = if result == b"true" {
        BooleanObject::True
    } else if result == b"false" {
        BooleanObject::False
    } else {
        unreachable!();
    };
    Ok((input, ret))
}
#[test]
fn parse_boolean_true() {
    let (rest, result) = object_boolean(b"trueasdf").unwrap();
    assert_eq!(rest, b"asdf");
    assert_eq!(result, BooleanObject::True);
}
#[test]
fn parse_boolean_false() {
    let (rest, result) = object_boolean(b"falseasdf").unwrap();
    assert_eq!(rest, b"asdf");
    assert_eq!(result, BooleanObject::False);
}
#[test]
fn parse_boolean_none() {
    let err = object_boolean(b"asdf");
    assert!(err.is_err());
}

// =====================
// 7.3.3 Numeric Objects
// =====================
// Store the sign separately, to be able to put it back.
#[derive(Debug)]
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
fn parse_sign(input: &[u8]) -> IResult<&[u8], Sign> {
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
#[derive(Debug)]
struct Integer<'a> {
    sign: Sign,
    digits: &'a [u8],
}
impl Serialize for Integer<'_> {
    fn serialize(&self, mut buf: &mut [u8]) {
        write!(buf, "{}", self.sign)
            .and(buf.write_all(self.digits))
            .unwrap();
    }
}
fn object_numeric_integer(input: &[u8]) -> IResult<&[u8], Integer> {
    let (input, (sign, digits)) = tuple((parse_sign, digit1))(input)?;
    // let value = i64::from_str_radix(digits, 10).unwrap();
    Ok((input, Integer { sign, digits }))
}

#[derive(Debug)]
struct Real<'a> {
    sign: Sign,
    digits_before: &'a [u8],
    digits_after: &'a [u8],
}
impl Serialize for Real<'_> {
    fn serialize(&self, mut buf: &mut [u8]) {
        // buf.write_all(format!("{}", self.sign).as_bytes());
        write!(buf, "{}", self.sign)
            .and(buf.write_all(self.digits_before))
            .and(buf.write_all(b"."))
            .and(buf.write_all(self.digits_after))
            .unwrap();
    }
}
fn object_numeric_real(input: &[u8]) -> IResult<&[u8], Real> {
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
#[derive(Debug)]
enum NumericObject<'a> {
    Integer(Integer<'a>),
    Real(Real<'a>),
}
impl Serialize for NumericObject<'_> {
    fn serialize(&self, buf: &mut [u8]) {
        match self {
            NumericObject::Integer(i) => i.serialize(buf),
            NumericObject::Real(r) => r.serialize(buf),
        }
    }
}

fn object_numeric(input: &[u8]) -> IResult<&[u8], NumericObject> {
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
#[derive(Debug)]
enum LiteralStringPart<'a> {
    Regular(&'a [u8]), // A part without a backslash
    Escaped(&'a [u8]), // The part after the backslash. 11 possibilities: \n \r \t \b \f \( \) \\ \oct \EOL or empty (e.g. in \a \c \d \e \g \h \i \j etc.)
}
#[derive(Debug)]
struct LiteralString<'a> {
    parts: Vec<LiteralStringPart<'a>>,
}
// Examples of literal strings:
// (abc)          => parts: [Regular("abc")]
// (ab (c) d)     => parts: [Regular("ab (c) d")]
// (\n c)         => parts: [Escaped("n"), Regular("c")]
// (ab ( \n c) d) => parts: ["Regular("ab ( ", Escaped("n"), Regular("c) d")]
impl Serialize for LiteralString<'_> {
    fn serialize(&self, mut buf: &mut [u8]) {
        buf.write_all(b"(").unwrap();
        for part in &self.parts {
            match part {
                LiteralStringPart::Regular(part) => buf.write_all(part),
                LiteralStringPart::Escaped(part) => buf.write_all(b"\\").and(buf.write_all(part)),
            }
            .unwrap();
        }
        buf.write_all(b")").unwrap();
    }
}

fn eol_marker(input: &[u8]) -> IResult<&[u8], &[u8]> {
    alt((tag(b"\r\n"), tag(b"\r"), tag(b"\n")))(input)
}

fn parse_escape(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let first = input[0];
    // The 8 single-char escapes: \n \r \t \b \f \( \) \\
    if b"nrtbf()\\".contains(&first) {
        Ok((&input[1..], &input[..1]))
    } else {
        // Three more cases: end-of-line marker, or 1 to 3 octal digits, or empty (=0 octal digits)
        eol_marker(input).or(
            //
            take_while_m_n(0, 3, is_oct_digit)(input),
        )
    }
}

// When *parsing*, '(' and ')' and '\' have special meanings.
fn object_literal_string<'a>(input: &'a [u8]) -> IResult<&[u8], LiteralString> {
    let (input, _) = tag(b"(")(input)?;
    let mut paren_depth = 1;
    let mut parts: Vec<LiteralStringPart<'a>> = vec![];
    let mut i = 0;
    let mut j = 0;
    loop {
        // No more characters. We should never get here, because final closing paren should be seen first.
        if j == input.len() {
            return Err(nom::Err::Incomplete(nom::Needed::Size(
                std::num::NonZeroUsize::new(paren_depth).unwrap(),
            )));
        }
        let c = input[j];
        if c == b'\\' {
            // Add any remaining leftovers, before adding the escaped part.
            if i < j {
                parts.push(LiteralStringPart::Regular(&input[i..j]));
            }
            j += 1;
            let (remaining_input, parsed_escape) = parse_escape(&input[j..]).unwrap();
            assert_eq!(
                remaining_input.len() + parsed_escape.len(),
                input[j..].len()
            );
            parts.push(LiteralStringPart::Escaped(parsed_escape));
            j += parsed_escape.len();
            i = j;
        } else if c == b'(' {
            paren_depth += 1;
            j += 1;
        } else if c == b')' {
            paren_depth -= 1;
            if paren_depth == 0 {
                // End of the string. Return.
                if i < j {
                    parts.push(LiteralStringPart::Regular(&input[i..j]));
                }
                return Ok((&input[j + 1..], LiteralString { parts }));
            }
            // Regular close-paren, goes to the end of current part.
            j += 1;
        } else {
            j += 1;
        }
    }
}

#[derive(Debug)]
enum StringObject<'a> {
    Literal(LiteralString<'a>),
}
impl Serialize for StringObject<'_> {
    fn serialize(&self, buf: &mut [u8]) {
        match self {
            StringObject::Literal(s) => s.serialize(buf),
        }
    }
}

// ===========
// 7.3 Objects
// ===========
#[derive(Debug)]
enum Object<'a> {
    Boolean(BooleanObject),
    Numeric(NumericObject<'a>),
    String(StringObject<'a>),
}
impl Serialize for Object<'_> {
    fn serialize(&self, buf: &mut [u8]) {
        match self {
            Object::Boolean(b) => b.serialize(buf),
            Object::Numeric(n) => n.serialize(buf),
            Object::String(s) => s.serialize(buf),
        }
    }
}

fn object(input: &[u8]) -> IResult<&[u8], Object> {
    alt((
        map(object_boolean, |b| Object::Boolean(b)),
        map(object_numeric, |n| Object::Numeric(n)),
        map(object_literal_string, |s| {
            Object::String(StringObject::Literal(s))
        }),
    ))(input)
    // let try_boolean = object_boolean(input);
    // let (input, object) = match try_boolean {
    //     Ok((input, result)) => (input, Object::Boolean(result)),
    //     Err(_) => {
    //         let (input, result) = object_numeric(input)?;
    //         (input, Object::Numeric(result))
    //     }
    // };
    // Ok((input, object))
}

// https://stackoverflow.com/a/42067321
pub fn str_from_u8_nul_utf8(utf8_src: &[u8]) -> Result<&str, std::str::Utf8Error> {
    let nul_range_end = utf8_src
        .iter()
        .position(|&c| c == b'\0')
        .unwrap_or(utf8_src.len()); // default to length if no `\0` present
    ::std::str::from_utf8(&utf8_src[0..nul_range_end])
}

#[test]
fn round_trip() {
    for input in [
        // from the spec
        "true",
        "false",
        // from the spec
        "123",
        "43445",
        "+17",
        "-98",
        "0",
        // with leading 0s
        "0042",
        "-0042",
        // from the spec
        "34.5",
        "-3.62",
        "+123.6",
        "4.",
        "-.002",
        "0.0",
        // from the spec
        "(This is a string)",
        "(Strings may contain newlines
        and such.)",
        "(Strings may contain balanced parentheses ( ) and
        special characters (*!&}^% and so on).)",
        "(The following is an empty string.)",
        "()",
        "(It has zero (0) length.)",
        "(These \\
            two strings \\
            are the same.)",
        "(These two strings are the same.)",
        "(This string has an end-of-line at the end of it.
        )",
        "(So does this one.\\n)",
        "(This string contains \\245two octal characters\\307.)",
        "(\\0053)",
        "(\\053)",
        "(\\53)",
        // More tricky examples
        "(abc)",
        "(ab (c) d)",
        "(\\n c)",
        "(ab ( \\n c) d)",
        "(ab \\c ( \\n d) e)",
    ] {
        let (remaining, result) = object(input.as_bytes()).unwrap();
        println!("{:?}", result);
        assert_eq!(remaining, b"");
        let mut buf = [0; 300];
        result.serialize(&mut buf);
        let out = str_from_u8_nul_utf8(&buf).unwrap();
        println!("{} vs {}", input, out);
        assert_eq!(input, out);
    }
}
