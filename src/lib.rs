use js_sys::Uint8Array;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1, take_while_m_n},
    character::{
        complete::{digit0, digit1, one_of},
        is_oct_digit,
    },
    combinator::{map, opt},
    multi::many0,
    sequence::{delimited, tuple},
    IResult,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    io::{self, Write},
};
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
pub trait BinSerialize {
    // This ought to *append* to buf.
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()>;
}

macro_rules! test_round_trip {
    ($name:ident: $s:expr) => {
        #[test]
        fn $name() {
            test_round_trip_str($s);
        }
    };
}

macro_rules! test_round_trip_b {
    ($name:ident: $s:expr) => {
        #[test]
        fn $name() {
            test_round_trip_bytes($s);
        }
    };
}

// =====================
// 7.3.2 Boolean Objects
// =====================
#[derive(Serialize, Deserialize, Debug)]
pub enum BooleanObject {
    True,
    False,
}
impl BinSerialize for BooleanObject {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_all(match self {
            BooleanObject::True => b"true",
            BooleanObject::False => b"false",
        })
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
    let serialized = serde_json::to_string(&result).unwrap();
    println!("Serialized as #{}#", serialized);
    match result {
        BooleanObject::True => assert!(true),
        BooleanObject::False => assert!(false),
    }
}
#[test]
fn parse_boolean_false() {
    let (rest, result) = object_boolean(b"falseasdf").unwrap();
    assert_eq!(rest, b"asdf");
    match result {
        BooleanObject::True => assert!(false),
        BooleanObject::False => assert!(true),
    }
}
#[test]
fn parse_boolean_none() {
    let err = object_boolean(b"asdf");
    assert!(err.is_err());
}

// From the spec
test_round_trip!(bool1: "true");
test_round_trip!(bool2: "false");

// =====================
// 7.3.3 Numeric Objects
// =====================
// Store the sign separately, to be able to put it back.
#[derive(Serialize, Deserialize, Debug)]
pub enum Sign {
    Plus,
    Minus,
    None,
}
impl BinSerialize for Sign {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        write!(
            buf,
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
#[derive(Serialize, Deserialize, Debug)]
pub struct Integer<'a> {
    sign: Sign,
    digits: Cow<'a, [u8]>,
}

impl BinSerialize for Integer<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        self.sign.serialize_to(buf)?;
        buf.write_all(&self.digits)
    }
}

fn object_numeric_integer(input: &[u8]) -> IResult<&[u8], Integer> {
    let (input, (sign, digits)) = tuple((parse_sign, digit1))(input)?;
    // let value = i64::from_str_radix(digits, 10).unwrap();
    Ok((
        input,
        Integer {
            sign,
            digits: Cow::Borrowed(digits),
        },
    ))
}

// from the spec
test_round_trip!(num101: "123");
test_round_trip!(num102: "43445");
test_round_trip!(num103: "+17");
test_round_trip!(num104: "-98");
test_round_trip!(num105: "0");
// with leading 0s
test_round_trip!(num106: "0042");
test_round_trip!(num107: "-0042");

#[test]
fn test_serde_num() {
    let input = "123";
    println!("Testing with input: #{}#", input);

    let parsed_object = object_numeric_integer(input.as_bytes());
    if parsed_object.is_err() {
        println!("Error parsing into object: {:?}", parsed_object);
    }
    let (remaining, result) = parsed_object.unwrap();
    println!("Parsed into object: {:?}", result);
    assert_eq!(remaining, b"");

    let serialized = serde_json::to_string(&result).unwrap();
    println!("Serialized into: #{}#", serialized);
    let deserialized: Integer = serde_json::from_str(&serialized).unwrap();
    let result = deserialized;

    let mut buf: Vec<u8> = vec![];
    result.serialize_to(&mut buf).unwrap();

    let out = str_from_u8_nul_utf8(&buf).unwrap();
    println!("{} vs {}", input, out);
    assert_eq!(input, out);
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Real<'a> {
    sign: Sign,
    digits_before: Cow<'a, [u8]>,
    digits_after: Cow<'a, [u8]>,
}
impl BinSerialize for Real<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        // buf.write_all(format!("{}", self.sign).as_bytes());
        self.sign.serialize_to(buf)?;
        buf.write_all(&self.digits_before)
            .and(buf.write_all(b"."))
            .and(buf.write_all(&self.digits_after))
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
            digits_before: Cow::Borrowed(digits_before),
            digits_after: Cow::Borrowed(digits_after),
        },
    ))
}
#[derive(Serialize, Deserialize, Debug)]
pub enum NumericObject<'a> {
    Integer(Integer<'a>),
    Real(Real<'a>),
}
impl BinSerialize for NumericObject<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            NumericObject::Integer(i) => i.serialize_to(buf),
            NumericObject::Real(r) => r.serialize_to(buf),
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

// from the spec
test_round_trip!(num201: "34.5");
test_round_trip!(num202: "-3.62");
test_round_trip!(num203: "+123.6");
test_round_trip!(num204: "4.");
test_round_trip!(num205: "-.002");
test_round_trip!(num206: "0.0");

// =====================
// 7.3.4 String Objects
// =====================

// 7.3.4.2 Literal Strings
// Sequence of bytes, where only \ has special meaning. (Note: When encoding, also need to escape unbalanced parentheses.)
// To be able to round-trip successfully, we'll store it as alternating sequences of <part before \, the special part after \>
#[derive(Serialize, Deserialize, Debug)]
enum LiteralStringPart<'a> {
    Regular(Cow<'a, [u8]>), // A part without a backslash
    Escaped(Cow<'a, [u8]>), // The part after the backslash. 11 possibilities: \n \r \t \b \f \( \) \\ \oct \EOL or empty (e.g. in \a \c \d \e \g \h \i \j etc.)
}
#[derive(Serialize, Deserialize, Debug)]
pub struct LiteralString<'a> {
    #[serde(borrow)]
    parts: Vec<LiteralStringPart<'a>>,
}
// Examples of literal strings:
// (abc)          => parts: [Regular("abc")]
// (ab (c) d)     => parts: [Regular("ab (c) d")]
// (\n c)         => parts: [Escaped("n"), Regular("c")]
// (ab ( \n c) d) => parts: ["Regular("ab ( ", Escaped("n"), Regular("c) d")]
impl BinSerialize for LiteralString<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_all(b"(")?;
        for part in &self.parts {
            match part {
                LiteralStringPart::Regular(part) => buf.write_all(part),
                LiteralStringPart::Escaped(part) => buf.write_all(b"\\").and(buf.write_all(part)),
            }?
        }
        buf.write_all(b")")
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
                parts.push(LiteralStringPart::Regular(Cow::Borrowed(&input[i..j])));
            }
            j += 1;
            let (remaining_input, parsed_escape) = parse_escape(&input[j..])?;
            assert_eq!(
                remaining_input.len() + parsed_escape.len(),
                input[j..].len()
            );
            parts.push(LiteralStringPart::Escaped(Cow::Borrowed(parsed_escape)));
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
                    parts.push(LiteralStringPart::Regular(Cow::Borrowed(&input[i..j])));
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

// from the spec
test_round_trip!(str101: "(This is a string)");
test_round_trip!(str102: "(Strings may contain newlines
                           and such.)");
test_round_trip!(str103: "(Strings may contain balanced parentheses ( ) and
      special characters (*!&}^% and so on).)");
test_round_trip!(str104: "(The following is an empty string.)");
test_round_trip!(str105: "()");
test_round_trip!(str106: "(It has zero (0) length.)");
test_round_trip!(str107: "(These \\);
                           two strings \\);
                           are the same.)");
test_round_trip!(str108: "(These two strings are the same.)");
test_round_trip!(str109: "(This string has an end-of-line at the end of it.
)");
test_round_trip!(str110: "(So does this one.\\n)");
test_round_trip!(str111: "(This string contains \\245two octal characters\\307.)");
test_round_trip!(str112: "(\\0053)");
test_round_trip!(str113: "(\\053)");
test_round_trip!(str114: "(\\53)");
// More tricky examples
test_round_trip!(str115: "(abc)");
test_round_trip!(str116: "(ab (c) d)");
test_round_trip!(str117: "(\\n c)");
test_round_trip!(str118: "(ab ( \\n c) d)");
test_round_trip!(str119: "(ab \\c ( \\n d) e)");
// Examples with non-printable chars and non-UTF bytes
test_round_trip_b!(str301: b"( \x80 \x99 \xFF )");

// 7.3.4.3 Hexadecimal Strings

// A character that can occur inside the <...> in a hexadecimal string.
fn is_hex_string_char(c: u8) -> bool {
    assert_eq!(0x20, b' '); // SPACE
    assert_eq!(0x09, b'\t'); // HORIZONTAL TAB
    assert_eq!(0x0D, b'\r'); // CARRIAGE RETURN
    assert_eq!(0x0A, b'\n'); // LINE FEED
    b'0' <= c && c <= b'9'
        || b'a' <= c && c <= b'f'
        || b'A' <= c && c <= b'F'
        || [0x20, 0x09, 0x0D, 0x0A, 0x0C].contains(&c)
}

// Example:
// <901FA3>  -> parts ['9', '0', '1', 'F', 'A', '3']
// <90 1fa>   -> parts ['9', '0', ' ', '1', 'f', 'a']
#[derive(Serialize, Deserialize, Debug)]
pub struct HexadecimalString<'a> {
    chars: Cow<'a, [u8]>,
}
impl BinSerialize for HexadecimalString<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_all(b"<")
            .and(buf.write_all(&self.chars))
            .and(buf.write_all(b">"))
    }
}
fn object_hexadecimal_string(input: &[u8]) -> IResult<&[u8], HexadecimalString> {
    map(
        delimited(tag(b"<"), take_while(is_hex_string_char), tag(b">")),
        |chars| HexadecimalString {
            chars: Cow::Borrowed(chars),
        },
    )(input)
}

// From spec
test_round_trip!(str201: "<4E6F762073686D6F7A206B6120706F702E>");
test_round_trip!(str202: "<901FA3>");
test_round_trip!(str203: "<901FA>");
// Add spaces etc.
test_round_trip!(str204: "<90 1f \r \n
             A>"
);

#[derive(Serialize, Deserialize, Debug)]
pub enum StringObject<'a> {
    #[serde(borrow)]
    Literal(LiteralString<'a>),
    Hex(HexadecimalString<'a>),
}
impl BinSerialize for StringObject<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            StringObject::Literal(s) => s.serialize_to(buf),
            StringObject::Hex(h) => h.serialize_to(buf),
        }
    }
}
fn object_string(input: &[u8]) -> IResult<&[u8], StringObject> {
    alt((
        map(object_literal_string, |s| StringObject::Literal(s)),
        map(object_hexadecimal_string, |s| StringObject::Hex(s)),
    ))(input)
}

// ==================
// 7.3.5 Name Objects
// ==================
#[derive(Serialize, Deserialize, Debug)]
pub enum NameObjectPart {
    Regular(u8),
    NumberSignPrefixed(u8),
}
impl BinSerialize for NameObjectPart {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            NameObjectPart::Regular(n) => write!(buf, "{}", *n as char),
            NameObjectPart::NumberSignPrefixed(n) => write!(buf, "#{}", *n),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NameObject {
    chars: Vec<NameObjectPart>,
}
impl BinSerialize for NameObject {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_all(b"/")?;
        for char in &self.chars {
            char.serialize_to(buf)?
        }
        Ok(())
    }
}

fn is_white_space_char(c: u8) -> bool {
    // NUL, HT, LF, FF, CR, SP
    [0, 9, 10, 12, 13, 32].contains(&c)
}

fn is_delimiter_char(c: u8) -> bool {
    b"()<>[]{}/%".contains(&c)
}

fn is_regular_char(c: u8) -> bool {
    !is_white_space_char(c) && !is_delimiter_char(c)
}

fn is_regular_character_for_name(c: u8) -> bool {
    is_regular_char(c) && b'!' <= c && c <= b'~'
}

fn object_name(input: &[u8]) -> IResult<&[u8], NameObject> {
    let (inp, _solidus) = tag(b"/")(input)?;
    let mut i = 0;
    let mut ret: Vec<NameObjectPart> = vec![];
    while i < inp.len() {
        let c = inp[i];
        if !is_regular_character_for_name(c) {
            return Ok((&inp[i..], NameObject { chars: ret }));
        }
        if c == b'#' {
            let num = ((inp[i + 1] as u8) - ('0' as u8)) * 10 + ((inp[i + 2] as u8) - ('0' as u8));
            ret.push(NameObjectPart::NumberSignPrefixed(num));
            i += 3;
        } else {
            ret.push(NameObjectPart::Regular(c));
            i += 1;
        }
    }
    // unreachable!("Should have encountered end of name before end of input.");
    Ok((&inp[i..], NameObject { chars: ret }))
}

// From spec
test_round_trip!(name101: "/Name1");
test_round_trip!(name102: "/ASomewhatLongerName");
test_round_trip!(name103: "/A;Name_With-Various***Characters?");
test_round_trip!(name104: "/1.2");
test_round_trip!(name105: "/$$");
test_round_trip!(name106: "/@pattern");
test_round_trip!(name107: "/.notdef");
test_round_trip!(name108: "/lime#20Green");
test_round_trip!(name109: "/paired#28#29parentheses");
test_round_trip!(name110: "/The_Key_of_F#23_Minor");
test_round_trip!(name111: "/A#42");
// Examples with non-printable chars and non-UTF bytes
test_round_trip_b!(name201: b"/hello#80#32#99world");
test_round_trip_b!(name202: br"/backslash\isnotspecial");

// ===================
// 7.3.6 Array Objects
// ===================
#[derive(Serialize, Deserialize, Debug)]
enum ArrayObjectPart<'a> {
    #[serde(borrow)]
    Object(Object<'a>),
    Whitespace(Cow<'a, [u8]>),
}

impl BinSerialize for ArrayObjectPart<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            ArrayObjectPart::Object(o) => o.serialize_to(buf),
            ArrayObjectPart::Whitespace(w) => buf.write_all(w),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ArrayObject<'a> {
    #[serde(borrow)]
    parts: Vec<ArrayObjectPart<'a>>,
}

impl BinSerialize for ArrayObject<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        write!(buf, "[")?;
        for part in &self.parts {
            println!("Before writing this part {:?}: buf was #{:?}#", part, buf);
            part.serialize_to(buf)?;
            println!("Afer writing this part {:?}: buf is #{:?}#\n", part, buf);
        }
        write!(buf, "]")
    }
}

fn array_object_part(input: &[u8]) -> IResult<&[u8], ArrayObjectPart> {
    alt((
        map(object, |o| ArrayObjectPart::Object(o)),
        map(take_while1(is_white_space_char), |w| {
            ArrayObjectPart::Whitespace(Cow::Borrowed(w))
        }),
    ))(input)
}

fn object_array(input: &[u8]) -> IResult<&[u8], ArrayObject> {
    delimited(
        tag(b"["),
        map(many0(array_object_part), |parts| {
            println!("Got array parts: {:?}", parts);
            ArrayObject { parts }
        }),
        tag(b"]"),
    )(input)
}

// From spec
test_round_trip!(array101: "[549 3.14 false (Ralph) /SomeName]");
// implicit in spec
test_round_trip!(array102: "[]");
test_round_trip!(array103: "[true [(hello) /bye[[[]]]]]");

// ========================
// 7.3.7 Dictionary Objects
// ========================
#[derive(Serialize, Deserialize, Debug)]
enum DictionaryPart<'a> {
    Key(NameObject),
    #[serde(borrow)]
    Value(Object<'a>),
    Whitespace(Cow<'a, [u8]>),
}
impl BinSerialize for DictionaryPart<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            DictionaryPart::Key(name) => name.serialize_to(buf),
            DictionaryPart::Value(value) => value.serialize_to(buf),
            DictionaryPart::Whitespace(w) => buf.write_all(w),
        }
    }
}
fn dictionary_part(input: &[u8]) -> IResult<&[u8], DictionaryPart> {
    alt((
        map(object_name, |name| DictionaryPart::Key(name)),
        map(object, |value| DictionaryPart::Value(value)),
        map(take_while1(is_white_space_char), |w| {
            DictionaryPart::Whitespace(Cow::Borrowed(w))
        }),
    ))(input)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DictionaryObject<'a> {
    #[serde(borrow)]
    parts: Vec<DictionaryPart<'a>>,
}
impl BinSerialize for DictionaryObject<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_all(b"<<")?;
        for part in &self.parts {
            part.serialize_to(buf)?;
        }
        buf.write_all(b">>")
    }
}

fn object_dictionary(input: &[u8]) -> IResult<&[u8], DictionaryObject> {
    delimited(
        tag(b"<<"),
        map(many0(dictionary_part), |parts| {
            println!("Got dictionary parts: {:?}", parts);
            DictionaryObject { parts }
        }),
        tag(b">>"),
    )(input)
}

// From spec
test_round_trip!(dict101: "<< /Type /Example
/Subtype /DictionaryExample
/Version 0.01
/IntegerItem 12
/StringItem (a string)
/Subdictionary << /Item1 0.4
/Item2 true
/LastItem (not!)
/VeryLastItem (OK)
>>
>>");

// ===========
// 7.3 Objects
// ===========
#[derive(Serialize, Deserialize, Debug)]
pub enum Object<'a> {
    Boolean(BooleanObject),
    #[serde(borrow)]
    Numeric(NumericObject<'a>),
    String(StringObject<'a>),
    Name(NameObject),
    Array(ArrayObject<'a>),
    Dictionary(DictionaryObject<'a>),
}
impl BinSerialize for Object<'_> {
    fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
        match self {
            Object::Boolean(b) => b.serialize_to(buf),
            Object::Numeric(n) => n.serialize_to(buf),
            Object::String(s) => s.serialize_to(buf),
            Object::Name(name) => name.serialize_to(buf),
            Object::Array(arr) => arr.serialize_to(buf),
            Object::Dictionary(dict) => dict.serialize_to(buf),
        }
    }
}

pub fn object(input: &[u8]) -> IResult<&[u8], Object> {
    alt((
        map(object_boolean, |b| Object::Boolean(b)),
        map(object_numeric, |n| Object::Numeric(n)),
        map(object_string, |s| Object::String(s)),
        map(object_name, |n| Object::Name(n)),
        map(object_array, |a| Object::Array(a)),
        map(object_dictionary, |d| Object::Dictionary(d)),
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

#[cfg(test)]
// https://stackoverflow.com/a/42067321
pub fn str_from_u8_nul_utf8(utf8_src: &[u8]) -> Result<&str, std::str::Utf8Error> {
    let nul_range_end = utf8_src
        .iter()
        .position(|&c| c == b'\0')
        .unwrap_or(utf8_src.len()); // default to length if no `\0` present
    ::std::str::from_utf8(&utf8_src[0..nul_range_end])
}

#[cfg(test)]
// Parses the input bytes into an Object, then writes that Object back into bytes.
fn parse_and_write(input: &[u8]) -> Vec<u8> {
    let parsed_object = object(input);
    if parsed_object.is_err() {
        println!("Error parsing into object: {:?}", parsed_object);
    }
    let (remaining, result) = parsed_object.unwrap();
    println!("Parsed into object: {:?}", result);
    assert_eq!(remaining, b"");

    let serialized = serde_json::to_string(&result).unwrap();
    println!("Serialized into: #{}#", serialized);
    let deserialized: Object = serde_json::from_str(&serialized).unwrap();
    let result = deserialized;

    let mut buf: Vec<u8> = vec![];
    result.serialize_to(&mut buf).unwrap();
    buf
}

#[cfg(test)]
fn test_round_trip_str(input: &str) {
    println!("Testing with input: #{}#", input);
    let buf = parse_and_write(input.as_bytes());
    let out = str_from_u8_nul_utf8(&buf).unwrap();
    println!("{} vs {}", input, out);
    assert_eq!(input, out);
}

#[cfg(test)]
fn test_round_trip_bytes(input: &[u8]) {
    println!("Testing with input: #{:?}#", input);
    let out = parse_and_write(input);
    println!("{:?} vs {:?}", input, out);
    assert_eq!(input, out);
}
