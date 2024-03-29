// @<wasm
use js_sys::Uint8Array;
use pdf_file_parse::BinSerialize;
use wasm_bindgen::prelude::*;
use web_sys::{console, File, FileReaderSync};

/// The function that is called from JS.
/// Reads `file`, parses it, logs some stuff, and returns the parsed structure.
#[wasm_bindgen]
pub fn handle_file(file: File) -> JsValue {
    console::log_1(&format!("in Rust handle_file").into());
    // Read `file` into a Vec<u8> v
    let v: Vec<u8> = {
        let filereader = FileReaderSync::new().unwrap();
        // Warning: This read_as_array_buffer can't be changed to readAsBinaryString.
        let buffer = filereader.read_as_array_buffer(&file).unwrap();
        let view = Uint8Array::new(&buffer); // This is instant.
        console::log_1(&format!("read {} bytes to ArrayBuffer", view.byte_length()).into());
        let v = view.to_vec();
        v
    };
    console::log_1(&format!("copied into Vec<u8>, computing crc32").into());

    let parsed = match pdf_file_parse::pdf_file(&v) {
        Ok((remaining, parsed)) => {
            println!("Parsed file with #{}# bytes left", remaining.len());
            parsed
        }
        Err(e) => {
            panic!("Failed to parse input as PDF. Got error: {:?}", e);
        }
    };

    // Check round-tripping
    {
        let mut out: Vec<u8> = vec![];
        parsed.serialize_to(&mut out).unwrap();
        console::log_1(
            &format!(
                "written-out PdfFile has len {} and crc32 {} (vs {})",
                out.len(),
                crc32fast::hash(&out),
                crc32fast::hash(&v),
            )
            .into(),
        );
    }

    // Log count of obj def-s.
    {
        let mut count = 0;
        for bct in &parsed.body_crossref_trailers {
            for bodypart in &bct.body {
                match bodypart {
                    pdf_file_parse::BodyPart::ObjDef(_) => count += 1,
                    pdf_file_parse::BodyPart::Whitespace(_) => {}
                }
            }
        }
        console::log_1(&format!("Parsed PdfFile has {} obj defs.", count).into());
    }

    JsValue::from_serde(&parsed).unwrap()
}
// >@wasm

// @<file_parse_and_back
// TODO: Put this in bin.rs?
/// Parses `input` as a PDF file, and returns its serialization (hopefully identical to input).
/// Panics if parsing fails.
pub fn file_parse_and_back(input: &[u8]) -> Vec<u8> {
    let parsed = match pdf_file_parse::pdf_file(input) {
        Ok((remaining, parsed)) => {
            println!("Parsed file with #{}# bytes left", remaining.len());
            parsed
        }
        Err(e) => {
            panic!("Failed to parse input as PDF. Got error: {:?}", e);
        }
    };
    let mut buf: Vec<u8> = vec![];
    parsed.serialize_to(&mut buf).unwrap();
    buf
}
// >@file_parse_and_back

// @<mod_header
mod pdf_file_parse {
    use adorn::adorn;
    use lazy_static::lazy_static;
    use nom::{
        branch::alt,
        bytes::complete::{tag, take, take_until, take_while, take_while1, take_while_m_n},
        character::{
            complete::{digit0, digit1, one_of},
            is_digit, is_oct_digit,
        },
        combinator::{map, opt, recognize, verify},
        multi::{many0, many1},
        sequence::{delimited, tuple},
        IResult, Parser,
    };
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use std::{
        borrow::Cow,
        io::{self, Write},
        ops::Add,
    };
    // >@mod_header

    // @<tracing
    lazy_static! {
        // The current depth of the parse calls
        static ref DEPTH: Mutex<i32> = Mutex::new(0);
        // A vector of the number of ops at each depth.
        // E.g.
        //  -> object [1]
        //   -> real  [1, 1]
        //   <- real (no) [2]
        //   -> int   [2, 1]
        //   <- int (ok) [3]
        static ref COST: Mutex<Vec<i64>> = Mutex::new(vec![0]);
    }

    #[allow(dead_code)]
    fn traceable_parser_full<'a, T, F>(
        f: F,
        fn_name: &'static str,
        input: &'a [u8],
    ) -> IResult<&'a [u8], T>
    where
        F: Fn(&'a [u8]) -> IResult<&'a [u8], T>,
    {
        const MAX_LEN: usize = 35;
        assert!(fn_name.len() < MAX_LEN, "{}", fn_name);
        *DEPTH.lock() += 1;
        eprint!(
            "{} -> {:MAX_LEN$}",
            " ".repeat(std::cmp::max(0i32, DEPTH.lock().add(0)) as usize),
            fn_name
        );
        let padding = std::cmp::max(0, 30_i32 - DEPTH.lock().add(0));
        eprint!("{}", " ".repeat(padding.try_into().unwrap()));
        eprint!("                    ");
        let prefix = &input[..std::cmp::min(input.len(), 70)];
        let prefix_for_debug = &input[..std::cmp::min(input.len(), 31)];
        match std::str::from_utf8(prefix) {
            Ok(s) => eprintln!("    {:?}", s),
            Err(_) => eprintln!("    {:?}", prefix_for_debug),
        }
        COST.lock().push(1);

        let ret = f(input);

        let mut costs = COST.lock();
        eprint!(
            "{} <- {:MAX_LEN$}",
            " ".repeat(std::cmp::max(0i32, DEPTH.lock().add(0)) as usize),
            fn_name
        );
        eprint!("{}", " ".repeat(padding.try_into().unwrap()));
        eprint!(
            "{} (after {:06} ops)",
            match ret {
                Ok(_) => "ok",
                Err(_) => "no",
            },
            costs.last().unwrap()
        );
        match std::str::from_utf8(prefix) {
            Ok(s) => eprint!("    {:?}", s),
            Err(_) => eprint!("    {:?}", prefix_for_debug),
        }
        if let Ok((left, _)) = ret {
            let prefix = &left[..std::cmp::min(left.len(), 30)];
            let prefix_for_debug = &left[..std::cmp::min(left.len(), 10)];
            match std::str::from_utf8(prefix) {
                Ok(s) => eprint!("    {:?}", s),
                Err(_) => eprint!("    {:?}", prefix_for_debug),
            }
        }
        eprintln!("");
        let current = costs.pop().unwrap();
        if let Some(_) = costs.last() {
            let v = costs.pop().unwrap();
            costs.push(v + current);
        }
        *DEPTH.lock() -= 1;

        ret
    }
    #[allow(dead_code)]
    fn traceable_parser_fast<'a, T, F>(
        f: F,
        _fn_name: &'static str,
        input: &'a [u8],
    ) -> IResult<&'a [u8], T>
    where
        F: Fn(&'a [u8]) -> IResult<&'a [u8], T>,
    {
        f(input)
    }

    fn traceable_parser<'a, T, F>(
        f: F,
        fn_name: &'static str,
        input: &'a [u8],
    ) -> IResult<&'a [u8], T>
    where
        F: Fn(&'a [u8]) -> IResult<&'a [u8], T>,
    {
        #[cfg(debug_assertions)]
        {
            traceable_parser_full(f, fn_name, input)
        }
        #[cfg(not(debug_assertions))]
        {
            traceable_parser_fast(f, fn_name, input)
        }
    }
    // >@tracing

    // @<BinSerialize
    // A trait for being able to serialize a type to bytes.
    pub trait BinSerialize {
        // Append bytes to `buf`.
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()>;
    }
    // >@BinSerialize

    // @<TestRoundTrip
    #[cfg(test)]
    // Parses the input bytes into an Object, then writes that Object back into bytes.
    fn parse_and_write(input: &[u8]) -> Vec<u8> {
        let parsed_object = object(input);
        if parsed_object.is_err() {
            println!("Error parsing into object: {:?}", parsed_object);
        }
        let (remaining, result) = parsed_object.unwrap();
        println!("Parsed into object: {:?}", result);
        assert_eq!(remaining, b"", "Nothing should remain after parsing");

        let serialized = serde_json::to_string(&result).unwrap();
        println!("Serialized into: #{}#", serialized);
        let deserialized: Object = serde_json::from_str(&serialized).unwrap();
        println!("Deserialized into: #{:?}#", deserialized);
        let result = deserialized;

        let mut buf: Vec<u8> = vec![];
        result.serialize_to(&mut buf).unwrap();
        buf
    }

    #[cfg(test)]
    fn test_round_trip_str(input: &str) {
        println!("Testing with input: #{}#", input);
        let buf = parse_and_write(input.as_bytes());
        let out = std::str::from_utf8(&buf).unwrap();
        println!("{} vs {}", input, out);
        assert_eq!(input, out);
    }

    #[cfg(test)]
    fn test_round_trip_bytes(input: &[u8]) {
        println!("Testing with input: #{:?}#", input);
        let out = parse_and_write(input);
        println!("{:?} vs {:?}", input, out);
        assert_eq!(input, out, "Round trip failed");
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
    // >@TestRoundTrip

    // =====================
    // 7.3.2 Boolean Objects
    // =====================
    // @<bool/enum
    #[derive(Serialize, Deserialize, Debug)]
    pub enum BooleanObject {
        True,
        False,
    }
    // >@bool/enum
    // @<bool/BinSerialize
    impl BinSerialize for BooleanObject {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(match self {
                BooleanObject::True => b"true",
                BooleanObject::False => b"false",
            })
        }
    }
    // >@bool/BinSerialize
    // @<bool/Parse
    fn object_boolean(input: &[u8]) -> IResult<&[u8], BooleanObject> {
        alt((
            tag("true").map(|_| BooleanObject::True),
            tag("false").map(|_| BooleanObject::False),
        ))(input)
    }
    // >@bool/Parse
    // @<bool/Tests
    #[test]
    fn parse_boolean_true() {
        let (rest, result) = object_boolean(b"trueasdf").unwrap();
        assert_eq!(rest, b"asdf");
        assert!(matches!(result, BooleanObject::True));
    }
    #[test]
    fn parse_boolean_false() {
        let (rest, result) = object_boolean(b"falseasdf").unwrap();
        assert_eq!(rest, b"asdf");
        assert!(matches!(result, BooleanObject::False));
    }
    #[test]
    fn parse_boolean_none() {
        let err = object_boolean(b"asdf");
        assert!(err.is_err());
    }

    // Examples listed in the spec
    test_round_trip!(bool1: "true");
    test_round_trip!(bool2: "false");
    // >@bool/Tests

    // =====================
    // 7.3.3 Numeric Objects
    // =====================
    // @<numeric/integer/sign
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
        map(opt(one_of("+-")), |sign| match sign {
            None => Sign::None,
            Some('+') => Sign::Plus,
            Some('-') => Sign::Minus,
            _ => unreachable!("Already checked + or -"),
        })(input)
    }
    // >@numeric/integer/sign
    // @<numeric/integer/type
    // Store the digits rather than just an i64, to be able to round-trip leading 0s.
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Integer<'a> {
        sign: Sign,
        digits: Cow<'a, [u8]>,
    }
    // >@numeric/integer/type
    // @<numeric/integer

    impl BinSerialize for Integer<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.sign.serialize_to(buf)?;
            buf.write_all(&self.digits)
        }
    }

    fn object_numeric_integer(input: &[u8]) -> IResult<&[u8], Integer> {
        map(tuple((parse_sign, digit1)), |(sign, digits)| Integer {
            sign,
            digits: Cow::Borrowed(digits),
        })(input)
    }

    fn integer_without_sign(input: &[u8]) -> IResult<&[u8], Integer> {
        map(digit1, |digits| Integer {
            sign: Sign::None,
            digits: Cow::Borrowed(digits),
        })(input)
    }

    // Examples from the spec
    test_round_trip!(int1: "123");
    test_round_trip!(int2: "43445");
    test_round_trip!(int3: "+17");
    test_round_trip!(int4: "-98");
    test_round_trip!(int5: "0");
    // with leading 0s
    test_round_trip!(int6: "0042");
    test_round_trip!(int7: "-0042");

    #[test]
    // Tests serializing to JSON and back.
    fn test_serde_num() {
        let input = "123";

        let (remaining, result) =
            object_numeric_integer(input.as_bytes()).expect("Error parsing into object");
        assert_eq!(remaining, b"");

        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: Integer = serde_json::from_str(&serialized).unwrap();

        let mut buf: Vec<u8> = vec![];
        deserialized.serialize_to(&mut buf).unwrap();
        let out = std::str::from_utf8(&buf).unwrap();
        assert_eq!(input, out);
    }
    // >@numeric/integer

    // @<numeric/real
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Real<'a> {
        sign: Sign,
        digits_before: Cow<'a, [u8]>,
        digits_after: Cow<'a, [u8]>,
    }
    impl BinSerialize for Real<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.sign.serialize_to(buf)?;
            buf.write_all(&self.digits_before)?;
            buf.write_all(b".")?;
            buf.write_all(&self.digits_after)
        }
    }
    fn object_numeric_real(input: &[u8]) -> IResult<&[u8], Real> {
        map(
            tuple((
                parse_sign,
                digit0,
                nom::character::complete::char('.'),
                digit0,
            )),
            |(sign, digits_before, _, digits_after)| Real {
                sign,
                digits_before: Cow::Borrowed(digits_before),
                digits_after: Cow::Borrowed(digits_after),
            },
        )(input)
    }
    // Examples from the spec
    test_round_trip!(real1: "34.5");
    test_round_trip!(real2: "-3.62");
    test_round_trip!(real3: "+123.6");
    test_round_trip!(real4: "4.");
    test_round_trip!(real5: "-.002");
    test_round_trip!(real6: "0.0");
    // >@numeric/real
    // @<numeric
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

    #[adorn(traceable_parser("numeric"))]
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
    // >@numeric

    // =====================
    // 7.3.4 String Objects
    // =====================

    // 7.3.4.2 Literal Strings
    // A string is a sequence of bytes, where only \ has special meaning.
    // @<string/literal/repr
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
    // (\n c)         => parts: [Escaped("n"), Regular(" c")]
    // (ab (c) d)     => parts: [Regular("ab (c) d")]
    // (ab ( \n c) d) => parts: ["Regular("ab ( ", Escaped("n"), Regular(" c) d")]
    // NOTE: We assume that the Regular parts together have balanced parentheses, i.e. that their parentheses don't need escaping.
    // TODO: While `object_literal_string` ensures this condition, a `LiteralString` could also be created from Deserialize... maybe implement custom deserialization?
    impl BinSerialize for LiteralString<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(b"(")?;
            for part in &self.parts {
                match part {
                    LiteralStringPart::Regular(part) => buf.write_all(part),
                    LiteralStringPart::Escaped(part) => {
                        buf.write_all(b"\\")?;
                        buf.write_all(part)
                    }
                }?
            }
            buf.write_all(b")")
        }
    }
    // >@string/literal/repr

    // @<string/literal/tests
    // Examples from the spec
    test_round_trip!(str101: "(This is a string)");
    test_round_trip!(str102: "(Strings may contain newlines
                           and such.)");
    test_round_trip!(str103: "(Strings may contain balanced parentheses ( ) and
      special characters (*!&}^% and so on).)");
    test_round_trip!(str104: "(The following is an empty string.)");
    test_round_trip!(str105: "()");
    test_round_trip!(str106: "(It has zero (0) length.)");
    test_round_trip!(str107: r#"(These \
                           two strings \
                           are the same.)"#);
    test_round_trip!(str108: "(These two strings are the same.)");
    test_round_trip!(str109: "(This string has an end-of-line at the end of it.
)");
    test_round_trip!(str110: r#"(So does this one.\n)"#);
    test_round_trip!(str111: r#"(This string contains \245two octal characters\307.)"#);
    test_round_trip!(str112: r#"(\0053)"#);
    test_round_trip!(str113: r#"(\053)"#);
    test_round_trip!(str114: r#"(\53)"#);
    // More tricky examples
    test_round_trip!(str115: "(abc)");
    test_round_trip!(str116: "(ab (c) d)");
    test_round_trip!(str117: r#"(\n c)"#);
    test_round_trip!(str118: r#"(ab ( \n c) d)"#);
    test_round_trip!(str119: r#"(ab \c ( \n d) e)"#);
    // Examples with non-printable chars and non-UTF bytes.
    // Note the below is *not* a raw string literal so escapes are interpreted by Rust,
    // so \x80 means the byte 128 in the string, etc.
    test_round_trip_b!(str301: b"( \x80 \x99 \xFF )");
    // >@string/literal/tests

    // @<string/literal/rest
    fn eol_any(input: &[u8]) -> IResult<&[u8], &[u8]> {
        alt((tag(b"\r\n"), tag(b"\r"), tag(b"\n")))(input)
    }

    // The escaped part that comes after a backslash.
    fn escaped_part(input: &[u8]) -> IResult<&[u8], &[u8]> {
        alt((
            // A single-char escape: \n \r \t \b \f \( \) \\
            verify(take(1usize), |byte: &[u8]| br"nrtbf()\".contains(&byte[0])),
            // A line break (end-of-line marker) following the backslash
            eol_any,
            // 1 to 3 octal digits
            take_while_m_n(1, 3, is_oct_digit),
            // Empty
            take(0usize),
        ))(input)
    }

    // Parses a string literal from `(` to `)`, while keeping track of balanced parentheses and handling backslash-escapes.
    // #[adorn(traceable_parser("literal_string"))]
    fn object_literal_string<'a>(input: &'a [u8]) -> IResult<&[u8], LiteralString> {
        let (input, _) = tag(b"(")(input)?;
        let mut parts: Vec<LiteralStringPart<'a>> = vec![]; // The result
        let mut paren_depth = 1;
        let mut i = 0; // Index of the first character that has not yet been added to the result, i.e. the number of characters that have been.
        let mut j = 0; // The "current" index (of the character we're about to examine).
        while j < input.len() {
            match input[j] {
                b'\\' => {
                    // Everything before this backslash constitutes a Regular part.
                    if i < j {
                        parts.push(LiteralStringPart::Regular(Cow::Borrowed(&input[i..j])));
                    }
                    j += 1;
                    let (remaining_input, parsed_escape) = escaped_part(&input[j..])?;
                    assert_eq!(
                        remaining_input.len() + parsed_escape.len(),
                        input[j..].len()
                    );
                    parts.push(LiteralStringPart::Escaped(Cow::Borrowed(parsed_escape)));
                    j += parsed_escape.len();
                    i = j;
                }
                b'(' => {
                    paren_depth += 1;
                    j += 1;
                }
                b')' => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        // End of the string. Return.
                        if i < j {
                            parts.push(LiteralStringPart::Regular(Cow::Borrowed(&input[i..j])));
                        }
                        return Ok((&input[j + 1..], LiteralString { parts }));
                    }
                    // We're at a close paren that does not end the string / current part.
                    j += 1;
                }
                _ => j += 1,
            }
        }
        // If we reach here (end of input), there were unmatched parentheses.
        Err(nom::Err::Incomplete(nom::Needed::Size(
            std::num::NonZeroUsize::new(paren_depth).unwrap(),
        )))
    }

    //>@string/literal
    // 7.3.4.3 Hexadecimal Strings

    //@<string/hexadecimal
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

    fn is_white_space_char(c: u8) -> bool {
        const NUL: u8 = 0;
        const HORIZONTAL_TAB: u8 = b'\t';
        const LINE_FEED: u8 = b'\n';
        const FORM_FEED: u8 = 0x0C;
        const CARRIAGE_RETURN: u8 = b'\r';
        const SPACE: u8 = b' ';
        match c {
            SPACE | HORIZONTAL_TAB | CARRIAGE_RETURN | LINE_FEED | NUL | FORM_FEED => true,
            _ => false,
        }
    }

    // A character that can occur inside the <...> in a hexadecimal string.
    fn is_hex_string_char(c: u8) -> bool {
        if is_white_space_char(c) {
            return true;
        }
        (b'0' <= c && c <= b'9') || (b'a' <= c && c <= b'f') || (b'A' <= c && c <= b'F')
    }
    fn object_hexadecimal_string(input: &[u8]) -> IResult<&[u8], HexadecimalString> {
        map(
            delimited(tag(b"<"), take_while(is_hex_string_char), tag(b">")),
            |chars| HexadecimalString {
                chars: Cow::Borrowed(chars),
            },
        )(input)
    }

    // Examples from the spec
    test_round_trip!(str201: "<4E6F762073686D6F7A206B6120706F702E>");
    test_round_trip!(str202: "<901FA3>");
    test_round_trip!(str203: "<901FA>");
    // Add spaces etc.
    test_round_trip!(str204: "<90 1f \r \n
                 A>"
    );
    // >@string/hexadecimal

    // @<string
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
    #[adorn(traceable_parser("string"))]
    fn object_string(input: &[u8]) -> IResult<&[u8], StringObject> {
        alt((
            map(object_literal_string, |s| StringObject::Literal(s)),
            map(object_hexadecimal_string, |s| StringObject::Hex(s)),
        ))(input)
    }
    // >@string

    // ==================
    // 7.3.5 Name Objects
    // ==================
    // @<name/repr
    #[derive(Serialize, Deserialize, Debug)]
    pub enum NameObjectChar {
        Regular(u8),
        NumberSignPrefixed(u8, u8),
    }
    impl BinSerialize for NameObjectChar {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                NameObjectChar::Regular(c) => buf.write_all(&[*c]),
                NameObjectChar::NumberSignPrefixed(n1, n2) => buf.write_all(&[b'#', *n1, *n2]),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct NameObject {
        chars: Vec<NameObjectChar>,
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
    // >@name/repr

    // @<name
    fn eof_error<I>(input: I) -> nom::Err<nom::error::Error<I>> {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Eof))
    }

    // #[adorn(traceable_parser("name"))]
    fn object_name(input: &[u8]) -> IResult<&[u8], NameObject> {
        let (mut rest, _solidus) = tag(b"/")(input)?;
        let mut chars: Vec<NameObjectChar> = vec![];
        while let Some(&c) = rest.first() {
            // Spec says characters outside printable ASCII range (! to ~) should also be written with #,
            // but in practice I see names like "/ABCDEE+等线,Bold" so stopping only on whitespace and delimiters.
            if is_white_space_char(c) || b"()<>[]{}/%".contains(&c) {
                break;
            }
            match c {
                b'#' => {
                    if rest.len() < 3 {
                        return Err(eof_error(rest));
                    }
                    chars.push(NameObjectChar::NumberSignPrefixed(rest[1], rest[2]));
                    rest = &rest[3..];
                }
                _ => {
                    chars.push(NameObjectChar::Regular(c));
                    rest = &rest[1..];
                }
            }
        }
        Ok((rest, NameObject { chars }))
    }

    // Examples from the spec
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
    // Real-life failure
    test_round_trip!(name301: "/AGSWKP#2bHelvetica");
    // "/ABCDEE+等线,Bold" -- not spec-compliant, but encountered in practice.
    test_round_trip_b!(name302: b"/ABCDEE+\xE7\xAD\x89\xE7\xBA\xBF,Bold");
    // >@name

    // ===================
    // 7.3.6 Array Objects
    // ===================
    // @<array/repr
    #[derive(Serialize, Deserialize, Debug)]
    enum ArrayObjectPart<'a> {
        #[serde(borrow)]
        ObjectOrRef(ObjectOrReference<'a>),
        Whitespace(Cow<'a, [u8]>),
    }

    impl BinSerialize for ArrayObjectPart<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                ArrayObjectPart::ObjectOrRef(o) => o.serialize_to(buf),
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
                part.serialize_to(buf)?;
            }
            write!(buf, "]")
        }
    }
    // >@array/repr

    // @<comments
    // #[adorn(traceable_parser("whitespace_and_comments"))]
    fn whitespace_and_comments(input: &[u8]) -> IResult<&[u8], &[u8]> {
        recognize(many0(alt((
            take_while1(is_white_space_char),
            recognize(tuple((
                tag(b"%"),
                take_while(|c| c != b'\n' && c != b'\r'),
                opt(alt((tag(b"\r\n"), tag(b"\n"), tag(b"\r")))),
            ))),
        ))))(input)
    }

    #[test]
    fn test_ws_simple() {
        let input = b"%PDF1.0\r%more\nleftover";
        let (remaining, _ws) = whitespace_and_comments(input).unwrap();
        assert_eq!(remaining, b"leftover");
    }

    #[test]
    fn test_ws_101() {
        let input: Vec<u8> = vec![
            37, 80, 68, 70, 45, 49, 46, 55, 13, 37, 200, 200, 200, 200, 200, 200, 200, 13, 49, 32,
            48,
        ];
        let (remaining, _ws) = whitespace_and_comments(&input).unwrap();

        assert_eq!(remaining, [49, 32, 48]);
    }

    fn whitespace_and_comments_nonempty(input: &[u8]) -> IResult<&[u8], &[u8]> {
        verify(whitespace_and_comments, |ws: &[u8]| !ws.is_empty())(input)
    }
    // >@comments

    // @<array/parse
    // #[adorn(traceable_parser("array_part"))]
    fn array_object_part(input: &[u8]) -> IResult<&[u8], ArrayObjectPart> {
        alt((
            map(object_or_ref, |o| ArrayObjectPart::ObjectOrRef(o)),
            map(whitespace_and_comments_nonempty, |w| {
                ArrayObjectPart::Whitespace(Cow::Borrowed(w))
            }),
        ))(input)
    }

    #[adorn(traceable_parser("array"))]
    fn object_array(input: &[u8]) -> IResult<&[u8], ArrayObject> {
        map(
            delimited(tag(b"["), many0(array_object_part), tag(b"]")),
            |parts| ArrayObject { parts },
        )(input)
    }

    // Example from the spec
    test_round_trip!(array101: "[549 3.14 false (Ralph) /SomeName]");
    // implicit in spec
    test_round_trip!(array102: "[]");
    test_round_trip!(array103: "[true [(hello) /bye[[[]]]]]");
    // >@array/parse

    // ========================
    // 7.3.7 Dictionary Objects
    // ========================
    // @<dict
    // A key and value, with optional whitespace between them.
    #[derive(Serialize, Deserialize, Debug)]
    struct KeyValuePair<'a> {
        key: NameObject,
        ws: Cow<'a, [u8]>,
        #[serde(borrow)]
        value: ObjectOrReference<'a>,
    }
    impl BinSerialize for KeyValuePair<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.key.serialize_to(buf)?;
            buf.write_all(&self.ws)?;
            self.value.serialize_to(buf)
        }
    }
    #[adorn(traceable_parser("dict_key_value_pair"))]
    fn key_value_pair(input: &[u8]) -> IResult<&[u8], KeyValuePair> {
        map(
            tuple((object_name, whitespace_and_comments, object_or_ref)),
            |(key, ws, value)| KeyValuePair {
                key,
                ws: Cow::Borrowed(ws),
                value,
            },
        )(input)
    }

    #[derive(Serialize, Deserialize, Debug)]
    enum DictionaryPart<'a> {
        Whitespace(Cow<'a, [u8]>),
        #[serde(borrow)]
        KeyValuePair(KeyValuePair<'a>),
    }
    impl BinSerialize for DictionaryPart<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                DictionaryPart::Whitespace(w) => buf.write_all(w),
                DictionaryPart::KeyValuePair(kv) => kv.serialize_to(buf),
            }
        }
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

    #[adorn(traceable_parser("dict"))]
    fn object_dictionary(input: &[u8]) -> IResult<&[u8], DictionaryObject> {
        let (input, (parts, final_ws)) = delimited(
            tag(b"<<"),
            tuple((
                many0(tuple((whitespace_and_comments, key_value_pair))),
                whitespace_and_comments,
            )),
            tag(b">>"),
        )(input)?;

        let mut dict_parts: Vec<DictionaryPart> = Vec::new();
        for (ws, pair) in parts {
            if !ws.is_empty() {
                dict_parts.push(DictionaryPart::Whitespace(Cow::Borrowed(ws)));
            }
            dict_parts.push(DictionaryPart::KeyValuePair(pair));
        }
        if !final_ws.is_empty() {
            dict_parts.push(DictionaryPart::Whitespace(Cow::Borrowed(final_ws)));
        }

        Ok((input, DictionaryObject { parts: dict_parts }))
    }

    test_round_trip!(dict_empty: "<<>>");
    test_round_trip!(dict_trivial: "<< /Key /Value /Key2 (Value2) >>");

    // #[test]
    // fn test_serialize() {
    //     let input = b"<</A /B>>";
    //     let (_, dict) = object_dictionary(input).unwrap();
    //     println!("Parsed into {:?}", dict.parts[0]);
    //     let kv_pair: &KeyValuePair = match &dict.parts[0] {
    //         DictionaryPart::Whitespace(_) => todo!(),
    //         DictionaryPart::KeyValuePair(pair) => pair,
    //     };
    //     let serialized = serde_json::to_string(&kv_pair).unwrap();
    //     println!("Serialized into: #{}#", serialized);
    //     let deserialized: KeyValuePair = serde_json::from_str(&serialized).unwrap();
    //     println!("Deserialized into: #{:?}#", deserialized);
    // }

    // From spec
    test_round_trip!(dict101: 
        "<< /Type /Example
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
    // Comment after "/Page"
    test_round_trip!(dict203: "<< /Type /Page % 1
/Parent 1 0 R
/MediaBox [ 0 0 60 11.25 ]
/Contents 4 0 R
/Group <<
   /Type /Group
   /S /Transparency
   /I true
   /CS /DeviceRGB
>>
/Resources 3 0 R
>>");
    // From real life, lightly modified. Note the "/companyName, LLC" as key!
    test_round_trip!(dict202: r"<<
    /Universal PDF(The process that creates this PDF ... United States)
    /Producer(pdfeTeX-1.21a; modified using iText� 5.5.6 �2000-2015 iText Group NV \(AGPL-version\))
    /Creator(TeX)
    /companyName, LLC(http://www.example.com)
    /ModDate(D:20170416015229+05'30')
    /CreationDate(D:20170331194508+02'00')
    >>");
    // >@dict

    // ====================
    // 7.3.8 Stream Objects
    // ====================

    // @<stream
    #[derive(Serialize, Deserialize, Debug)]
    enum EolMarker {
        CRLF,
        LF,
    }

    struct RestOfStreamObject<'a> {
        ws_and_comments: Cow<'a, [u8]>, // The whitespace (and comments) after the dict and before the stream
        eol_after_stream_begin: EolMarker, // The EOL marker (either CRLF or LF) after the "stream" keyword
        content: Cow<'a, [u8]>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct StreamObject<'a> {
        #[serde(borrow)]
        dict: DictionaryObject<'a>,
        ws_and_comments: Cow<'a, [u8]>, // The whitespace (and comments) after the dict and before the stream
        eol_after_stream_begin: EolMarker, // The EOL marker (either CRLF or LF) after the "stream" keyword
        content: Cow<'a, [u8]>,
    }

    impl std::fmt::Debug for StreamObject<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "StreamObject {{ ")?;
            // a comma-separated list of each field's name and Debug value
            write!(f, "dict: {:?}", &self.dict)?;
            write!(f, "ws_and_comments: {:?}", &self.ws_and_comments)?;
            write!(
                f,
                "eol_after_stream_begin: {:?}",
                &self.eol_after_stream_begin
            )?;
            write!(f, "content: ({} bytes)", &self.content.len())?;
            write!(f, " }}")
        }
    }

    impl BinSerialize for StreamObject<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.dict.serialize_to(buf)?;
            buf.write_all(&self.ws_and_comments)?;
            buf.write_all(b"stream")?;
            buf.write_all(match self.eol_after_stream_begin {
                EolMarker::CRLF => b"\r\n",
                EolMarker::LF => b"\n",
            })?;
            buf.write_all(&self.content)?;
            buf.write_all(b"endstream")
        }
    }

    #[adorn(traceable_parser("rest_of_stream"))]
    fn object_stream_after_dict(input: &[u8]) -> IResult<&[u8], RestOfStreamObject> {
        let (input, ws_and_comments) = whitespace_and_comments(input)?;
        // println!("Got some ws: {:?}", from_utf8(ws_and_comments).unwrap());
        let (input, _) = tag("stream")(input)?;
        // println!(
        //     "Also parsed 'stream': remaining is #{}# bytes which starts with {} and {}",
        //     input.len(),
        //     input[0],
        //     input[1]
        // );
        let (input, eol) = alt((tag(b"\r\n"), tag(b"\n")))(input)?;
        // println!("And the EOL marker following.");
        let eol_after_stream_begin = if eol == b"\r\n" {
            EolMarker::CRLF
        } else {
            EolMarker::LF
        };
        let (input, content) = take_until("endstream")(input)?;
        let (input, _) = tag("endstream")(input)?;
        Ok((
            input,
            RestOfStreamObject {
                ws_and_comments: Cow::Borrowed(ws_and_comments),
                eol_after_stream_begin,
                content: Cow::Borrowed(content),
            },
        ))
    }

    // Simplified from spec
    test_round_trip!(stream101: "<< /Length 42 >> % An indirect reference to object 8
stream
BT
/F1 12 Tf
72 712 Td
(A stream with an indirect length) Tj
ET
endstream");
    // Actual from spec
    test_round_trip!(stream102: "<< /Length 8 0 R >> % An indirect reference to object 8
stream
BT
/F1 12 Tf
72 712 Td
(A stream with an indirect length) Tj
ET
endstream");

    test_round_trip_b!(stream103: include_bytes!("test_4.in"));
    // >@stream

    // =================
    // 7.3.9 Null Object
    // =================
    // @<null
    test_round_trip!(null101: "null");
    // >@null

    // =======================
    // 7.3.10 Indirect Objects
    // =======================
    // @<indirect_object_reference
    #[derive(Serialize, Deserialize, Debug)]
    pub struct IndirectObjectReference<'a> {
        object_number: Integer<'a>,
        ws1: Cow<'a, [u8]>,
        generation_number: Integer<'a>,
        ws2: Cow<'a, [u8]>,
    }
    impl BinSerialize for IndirectObjectReference<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.object_number.serialize_to(buf)?;
            buf.write_all(&self.ws1)?;
            self.generation_number.serialize_to(buf)?;
            buf.write_all(&self.ws2)?;
            buf.write_all(b"R")
        }
    }
    #[adorn(traceable_parser("indirect_object_reference"))]
    fn indirect_object_reference(input: &[u8]) -> IResult<&[u8], IndirectObjectReference> {
        /*
        Note: You may think we can use "integer_without_sign" here, but I'm looking at a PDF that has:
            3 0 obj
            <<
              /Type /Outlines
              /Count 0
              /First -1 0 R
              /Last -1 0 R
            >>
            endobj
        */
        let (input, int1) = object_numeric_integer(input)?;
        let (input, ws1) = whitespace_and_comments(input)?;
        let (input, int2) = integer_without_sign(input)?;
        let (input, ws2) = whitespace_and_comments(input)?;
        let (input, _) = tag(b"R")(input)?;
        Ok((
            input,
            IndirectObjectReference {
                object_number: int1,
                ws1: Cow::Borrowed(ws1),
                generation_number: int2,
                ws2: Cow::Borrowed(ws2),
            },
        ))
    }
    // >@indirect_object_reference

    // @<indirect_object_definition
    #[derive(Serialize, Deserialize, Debug)]
    pub struct IndirectObjectDefinition<'a> {
        object_number: Integer<'a>,
        ws1: Cow<'a, [u8]>, // Between the object number and the generation number
        generation_number: Integer<'a>,
        ws2: Cow<'a, [u8]>, // Between the generation number and "def"
        ws3: Cow<'a, [u8]>, // Between "def" and the actual start of the object
        #[serde(borrow)]
        object: Object<'a>,
        ws4: Cow<'a, [u8]>, // Between the actual object and "endobj"
    }
    impl BinSerialize for IndirectObjectDefinition<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.object_number.serialize_to(buf)?;
            buf.write_all(&self.ws1)?;
            self.generation_number.serialize_to(buf)?;
            buf.write_all(&self.ws2)?;
            buf.write_all(b"obj")?;
            buf.write_all(&self.ws3)?;
            self.object.serialize_to(buf)?;
            buf.write_all(&self.ws4)?;
            buf.write_all(b"endobj")
        }
    }
    #[adorn(traceable_parser("indirect_object_definition"))]
    fn indirect_object_definition(input: &[u8]) -> IResult<&[u8], IndirectObjectDefinition> {
        // println!("Trying to parse obj def from {} bytes", input.len());
        let (input, int1) = integer_without_sign(input)?;
        // println!("int1 {:?} trying to parse from {} bytes", int1, input.len());
        let (input, ws1) = whitespace_and_comments(input)?;
        let (input, int2) = integer_without_sign(input)?;
        // println!("int2 {:?} trying to parse from {} bytes", int2, input.len());
        let (input, ws2) = whitespace_and_comments(input)?;
        let (input, _def) = tag(b"obj")(input)?;
        // println!("Reached def");
        let (input, ws3) = whitespace_and_comments(input)?;
        let (input, object) = object(input)?;
        let (input, ws4) = whitespace_and_comments(input)?;
        let (input, _endobj) = tag(b"endobj")(input)?;
        // println!("Reached endobj");
        let ret = IndirectObjectDefinition {
            object_number: int1,
            ws1: Cow::Borrowed(ws1),
            generation_number: int2,
            ws2: Cow::Borrowed(ws2),
            ws3: Cow::Borrowed(ws3),
            object,
            ws4: Cow::Borrowed(ws4),
        };
        let mut out: Vec<u8> = vec![];
        ret.serialize_to(&mut out).unwrap();
        println!(
            "Got an indirect object definition of {} bytes, with {} bytes left",
            out.len(),
            input.len()
        );
        Ok((input, ret))
    }
    // >@indirect_object_definition

    // ===========
    // 7.3 Objects
    // ===========
    // @<object
    #[derive(Serialize, Deserialize, Debug)]
    pub enum Object<'a> {
        Boolean(BooleanObject),
        #[serde(borrow)]
        Numeric(NumericObject<'a>),
        String(StringObject<'a>),
        Name(NameObject),
        Array(ArrayObject<'a>),
        Dictionary(DictionaryObject<'a>),
        Stream(StreamObject<'a>),
        Null,
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
                Object::Stream(stream) => stream.serialize_to(buf),
                Object::Null => buf.write_all(b"null"),
            }
        }
    }

    #[adorn(traceable_parser("object"))]
    pub fn object(input: &[u8]) -> IResult<&[u8], Object> {
        // Indirect way of returning on empty input with right error type
        let (_, first) = take(1usize)(input)?;
        if first == b"[" {
            map(object_array, |a| Object::Array(a))(input)
        } else if first == b"/" {
            map(object_name, |n| Object::Name(n))(input)
        } else if first == b"(" {
            map(object_literal_string, |s| {
                Object::String(StringObject::Literal(s))
            })(input)
        } else if first == b"<" {
            let (_, first_two) = take(2usize)(input)?;
            if first_two != b"<<" {
                map(object_hexadecimal_string, |s| {
                    Object::String(StringObject::Hex(s))
                })(input)
            } else {
                let (input, dict) = object_dictionary(input)?;
                match object_stream_after_dict(input) {
                    Ok((input, rest_of_stream)) => Ok((
                        input,
                        Object::Stream(StreamObject {
                            dict,
                            ws_and_comments: rest_of_stream.ws_and_comments,
                            eol_after_stream_begin: rest_of_stream.eol_after_stream_begin,
                            content: rest_of_stream.content,
                        }),
                    )),
                    Err(_) => Ok((input, Object::Dictionary(dict))),
                }
            }
        } else {
            alt((
                map(object_boolean, |b| Object::Boolean(b)),
                map(object_numeric, |n| Object::Numeric(n)),
                map(tag(b"null"), |_| Object::Null),
            ))(input)
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub enum ObjectOrReference<'a> {
        #[serde(borrow)]
        Object(Object<'a>),
        Reference(IndirectObjectReference<'a>),
    }
    impl BinSerialize for ObjectOrReference<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                ObjectOrReference::Object(o) => o.serialize_to(buf),
                ObjectOrReference::Reference(r) => r.serialize_to(buf),
            }
        }
    }

    #[adorn(traceable_parser("object_or_ref"))]
    pub fn object_or_ref(input: &[u8]) -> IResult<&[u8], ObjectOrReference> {
        alt((
            map(indirect_object_reference, |r| {
                ObjectOrReference::Reference(r)
            }),
            map(object, |o| ObjectOrReference::Object(o)),
        ))(input)
    }
    // >@object

    // ==================
    // 7.5 File Structure
    // ==================
    // @<body_part
    #[derive(Serialize, Deserialize, Debug)]
    pub enum BodyPart<'a> {
        #[serde(borrow)]
        ObjDef(IndirectObjectDefinition<'a>),
        Whitespace(Cow<'a, [u8]>),
    }
    impl BinSerialize for BodyPart<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                BodyPart::ObjDef(o) => o.serialize_to(buf),
                BodyPart::Whitespace(ws) => buf.write_all(ws),
            }
        }
    }
    #[adorn(traceable_parser("body_part"))]
    fn body_part(input: &[u8]) -> IResult<&[u8], BodyPart> {
        alt((
            map(indirect_object_definition, |def| BodyPart::ObjDef(def)),
            map(whitespace_and_comments, |ws| {
                BodyPart::Whitespace(Cow::Borrowed(ws))
            }),
        ))(input)
    }
    // >@body_part

    // @<cross_ref
    #[derive(Serialize, Deserialize, Debug)]
    enum CrossReferenceEntryInUse {
        Free,
        InUse,
    }
    #[derive(Serialize, Deserialize, Debug)]
    struct CrossReferenceEntry {
        nnnnnnnnnn: [u8; 10],
        ggggg: [u8; 5],
        n_or_f: CrossReferenceEntryInUse,
        eol: [u8; 2],
    }
    impl BinSerialize for CrossReferenceEntry {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(&self.nnnnnnnnnn)?;
            buf.write_all(b" ")?;
            buf.write_all(&self.ggggg)?;
            buf.write_all(b" ")?;
            buf.write_all(match self.n_or_f {
                CrossReferenceEntryInUse::Free => b"f",
                CrossReferenceEntryInUse::InUse => b"n",
            })?;
            buf.write_all(&self.eol)
        }
    }
    #[adorn(traceable_parser("cross_reference_subsection_entry"))]
    fn cross_reference_subsection_entry(input: &[u8]) -> IResult<&[u8], CrossReferenceEntry> {
        let (input, nnnnnnnnnn) = take_while_m_n(10, 10, is_digit)(input)?;

        let (input, _sp) = take(1usize)(input)?;
        assert_eq!(_sp, b" ");

        let (input, ggggg) = digit1(input)?;
        assert_eq!(ggggg.len(), 5);

        let (input, _sp) = take(1usize)(input)?;
        assert_eq!(_sp, b" ");

        let (input, n_or_f) = take(1usize)(input)?;
        assert!(n_or_f == b"n" || n_or_f == b"f");
        let n_or_f = if n_or_f == b"n" {
            CrossReferenceEntryInUse::InUse
        } else {
            CrossReferenceEntryInUse::Free
        };

        let (input, eol) = take(2usize)(input)?;
        let ret = CrossReferenceEntry {
            nnnnnnnnnn: nnnnnnnnnn.try_into().unwrap(),
            ggggg: ggggg.try_into().unwrap(),
            n_or_f,
            eol: eol.try_into().unwrap(),
        };
        Ok((input, ret))
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CrossReferenceSubsection<'a> {
        first_object_number: Integer<'a>, // The number of the first object in this subsection
        number_of_entries: Integer<'a>,   // How many objects this subsection is about
        ws: Cow<'a, [u8]>,                // After the first line (e.g. "28 5") of the subsection
        entries: Vec<CrossReferenceEntry>,
    }
    impl BinSerialize for CrossReferenceSubsection<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            // buf.write_all(b"xref")?;
            // buf.write_all(&self.ws1)?;
            self.first_object_number.serialize_to(buf)?;
            buf.write_all(b" ")?;
            self.number_of_entries.serialize_to(buf)?;
            buf.write_all(&self.ws)?;
            for entry in &self.entries {
                entry.serialize_to(buf)?;
            }
            buf.write_all(b"")
        }
    }
    #[adorn(traceable_parser("cross_reference_subsection"))]
    fn cross_reference_subsection(input: &[u8]) -> IResult<&[u8], CrossReferenceSubsection> {
        map(
            tuple((
                integer_without_sign,
                tag(b" "),
                integer_without_sign,
                whitespace_and_comments,
                many0(cross_reference_subsection_entry),
            )),
            |(n1, _sp, n2, ws, entries)| CrossReferenceSubsection {
                first_object_number: n1,
                number_of_entries: n2,
                ws: Cow::Borrowed(ws),
                entries,
            },
        )(input)
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CrossReferenceTable<'a> {
        ws1: Cow<'a, [u8]>, // The newline after "xref"
        subsections: Vec<CrossReferenceSubsection<'a>>,
        ws2: Cow<'a, [u8]>, // At the very end
    }
    impl BinSerialize for CrossReferenceTable<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(b"xref")?;
            buf.write_all(&self.ws1)?;
            for subsection in &self.subsections {
                subsection.serialize_to(buf)?;
            }
            buf.write_all(&self.ws2)
        }
    }
    #[adorn(traceable_parser("cross_reference_table"))]
    fn cross_reference_table(input: &[u8]) -> IResult<&[u8], CrossReferenceTable> {
        println!(
            "Trying to parse cross-reference table from {:?}",
            &input[..std::cmp::min(input.len(), 50)]
        );
        map(
            tuple((
                tag(b"xref"),
                whitespace_and_comments,
                many1(cross_reference_subsection),
                whitespace_and_comments,
            )),
            |(_xref, ws1, subsections, ws2)| CrossReferenceTable {
                ws1: Cow::Borrowed(ws1),
                subsections,
                ws2: Cow::Borrowed(ws2),
            },
        )(input)
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct StartxrefOffsetEof<'a> {
        ws3: Cow<'a, [u8]>, // After "startxref", before last penultimate line
        last_crossref_offset: Integer<'a>, // Byte offset of the last cross-reference section
        eol_marker: Cow<'a, [u8]>, // EOL after the byte offset
    }
    impl BinSerialize for StartxrefOffsetEof<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(b"startxref")?;
            buf.write_all(&self.ws3)?;
            self.last_crossref_offset.serialize_to(buf)?;
            buf.write_all(&self.eol_marker)?;
            buf.write_all(b"%%EOF")
        }
    }

    #[adorn(traceable_parser("eols"))]
    fn eol_markers_after_offset(input: &[u8]) -> IResult<&[u8], &[u8]> {
        let mut i = 0;
        while i < input.len() && input[i] == b' ' {
            i += 1;
        }
        // Hack because I haven't looked into how to return error properly.
        let (_, _) = take(1usize)(&input[i..])?;
        while i < input.len() && (input[i..].starts_with(b"\r") || input[i..].starts_with(b"\n")) {
            if input[i..].starts_with(b"\r\n") {
                i += 2;
            } else if input[i..].starts_with(b"\r") {
                i += 1;
            } else if input[i..].starts_with(b"\n") {
                i += 1;
            } else {
                unreachable!("Already checked for starting with \\r or \\n");
            }
        }
        Ok((&input[i..], &input[..i]))
    }

    #[adorn(traceable_parser("startxref_offset_eof"))]
    fn startxref_offset_eof(input: &[u8]) -> IResult<&[u8], StartxrefOffsetEof> {
        map(
            tuple((
                tag(b"startxref"),
                whitespace_and_comments,
                integer_without_sign,
                eol_markers_after_offset,
                tag(b"%%EOF"),
            )),
            |(_startxref, ws3, offset, eol, _eof)| StartxrefOffsetEof {
                ws3: Cow::Borrowed(ws3),
                last_crossref_offset: offset,
                eol_marker: Cow::Borrowed(eol),
            },
        )(input)
    }

    #[test]
    fn test_startxref_etc1() {
        let input = b"startxref\n442170 \n%%EOF";
        let (remaining, parsed) = startxref_offset_eof(input).unwrap();
        assert_eq!(remaining, b"", "Should be fully parsed");
        println!("{:?}", parsed);
    }
    // >@cross_ref

    // @<trailer
    #[derive(Serialize, Deserialize, Debug)]
    struct Trailer<'a> {
        ws1: Cow<'a, [u8]>, // After "trailer", before dict
        #[serde(borrow)]
        dict: DictionaryObject<'a>, // The actual trailer dictionary
        ws2: Cow<'a, [u8]>, // After dict, before "startxref"
    }
    impl BinSerialize for Trailer<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(b"trailer")?;
            buf.write_all(&self.ws1)?;
            self.dict.serialize_to(buf)?;
            buf.write_all(&self.ws2)
        }
    }
    #[adorn(traceable_parser("trailer"))]
    fn trailer(input: &[u8]) -> IResult<&[u8], Trailer> {
        map(
            tuple((
                tag(b"trailer"),
                whitespace_and_comments,
                object_dictionary,
                whitespace_and_comments,
            )),
            |(_trailer, ws1, dict, ws2)| Trailer {
                ws1: Cow::Borrowed(ws1),
                dict,
                ws2: Cow::Borrowed(ws2),
            },
        )(input)
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CrossReferenceTableAndTrailer<'a> {
        cross_reference_table: CrossReferenceTable<'a>,
        #[serde(borrow)]
        trailer: Trailer<'a>,
    }
    impl BinSerialize for CrossReferenceTableAndTrailer<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            self.cross_reference_table.serialize_to(buf)?;
            self.trailer.serialize_to(buf)
        }
    }
    fn cross_reference_table_and_trailer(
        input: &[u8],
    ) -> IResult<&[u8], CrossReferenceTableAndTrailer> {
        let (input, cross_reference_table) = cross_reference_table(input)?;
        let (input, trailer) = trailer(input)?;
        Ok((
            input,
            CrossReferenceTableAndTrailer {
                cross_reference_table,
                trailer,
            },
        ))
    }
    // >@trailer

    // @<body_crossref_trailer
    #[derive(Serialize, Deserialize, Debug)]
    pub struct BodyCrossrefTrailer<'a> {
        #[serde(borrow)]
        pub body: Vec<BodyPart<'a>>,
        cross_reference_table_and_trailer: Option<CrossReferenceTableAndTrailer<'a>>,
        startxref_offset_eof: StartxrefOffsetEof<'a>,
    }
    impl BinSerialize for BodyCrossrefTrailer<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            for part in &self.body {
                part.serialize_to(buf)?;
            }
            if let Some(t) = &self.cross_reference_table_and_trailer {
                t.serialize_to(buf)?
            }
            self.startxref_offset_eof.serialize_to(buf)
        }
    }
    #[adorn(traceable_parser("body_crossref_trailer"))]
    fn body_crossref_trailer(input: &[u8]) -> IResult<&[u8], BodyCrossrefTrailer> {
        println!(
            "Trying to parse b/c/t section from {} bytes starting {:?}",
            input.len(),
            &input[..std::cmp::min(input.len(), 20)]
        );
        let mut input = input;
        let mut body = vec![];
        loop {
            match body_part(input) {
                Ok((left, part)) => {
                    // println!(
                    //     "Parsed a body part ({}): now {} bytes left.",
                    //     match &part {
                    //         BodyPart::ObjDef(_) => "ObjDef".to_string(),
                    //         BodyPart::Whitespace(ws) => format!("Whitespace {:?}", ws),
                    //     },
                    //     left.len()
                    // );
                    input = left;
                    match part {
                        BodyPart::Whitespace(w) if w.len() == 0 => break,
                        x => body.push(x),
                    }
                }
                Err(_) => break,
            }
        }
        println!("{} objects; {} bytes left.", body.len(), input.len());

        // Two options: Either a cross-reference table, starting with "xref", or just the "startxref"...%%EOF
        let (input, cross_reference_table_and_trailer) =
            opt(cross_reference_table_and_trailer)(input)?;
        println!(
            "{} bytes left after cross-reference table and trailer",
            input.len(),
        );
        let (input, startxref_offset_eof) = startxref_offset_eof(input)?;
        Ok((
            input,
            BodyCrossrefTrailer {
                body,
                cross_reference_table_and_trailer,
                startxref_offset_eof,
            },
        ))
    }
    // >@body_crossref_trailer

    // @<pdf_file
    #[derive(Serialize, Deserialize)]
    pub struct PdfFile<'a> {
        header: Cow<'a, [u8]>,
        #[serde(borrow)]
        pub body_crossref_trailers: Vec<BodyCrossrefTrailer<'a>>,
        post_eof: Cow<'a, [u8]>,
    }
    impl BinSerialize for PdfFile<'_> {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            buf.write_all(&self.header)?;
            for bct in &self.body_crossref_trailers {
                bct.serialize_to(buf)?;
            }
            assert!(self.body_crossref_trailers.len() > 0);
            assert!(buf.ends_with(b"%%EOF"));
            buf.write_all(&self.post_eof)
        }
    }

    #[adorn(traceable_parser("pdf_file"))]
    pub fn pdf_file(input: &[u8]) -> IResult<&[u8], PdfFile> {
        let (input, header) = whitespace_and_comments(input)?;
        println!("{} bytes header, {} bytes left.", header.len(), input.len());

        let (input, bcts) = many1(body_crossref_trailer)(input)?;
        println!("After {} sections: {} bytes left.", bcts.len(), input.len());

        // Ideally, the remaining "input" won't contain any "%%EOF"
        let foo = input
            .windows(b"%%EOF".len())
            .position(|window| window == b"%%EOF");
        assert_eq!(foo, None);
        let (input, final_ws) = whitespace_and_comments(input)?;
        Ok((
            input,
            PdfFile {
                header: Cow::Borrowed(header),
                body_crossref_trailers: bcts,
                post_eof: Cow::Borrowed(final_ws),
            },
        ))
    }
}
// >@pdf_file
