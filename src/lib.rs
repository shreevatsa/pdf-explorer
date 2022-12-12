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

mod pdf_file_parse {
    use adorn::adorn;
    use lazy_static::lazy_static;
    use nom::{
        branch::alt,
        bytes::complete::{tag, take, take_until, take_while, take_while_m_n},
        character::{
            complete::{digit0, digit1, one_of},
            is_digit, is_oct_digit,
        },
        combinator::{map, opt},
        multi::{many0, many1, many_till},
        sequence::{delimited, tuple},
        IResult,
    };
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use std::{
        borrow::Cow,
        io::{self, Write},
        ops::Add,
    };

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

    type PdfBytes<'a> = &'a [u8];

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
        traceable_parser_fast(f, fn_name, input)
        // traceable_parser_full(f, fn_name, input)
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

    /* #region objects direct and indirect */
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
    // #[adorn(traceable_parser("boolean"))]
    fn object_boolean(input: PdfBytes) -> IResult<PdfBytes, BooleanObject> {
        alt((
            map(tag("true"), |_| BooleanObject::True),
            map(tag("false"), |_| BooleanObject::False),
        ))(input)
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

    // #[adorn(traceable_parser("sign"))]
    fn parse_sign(input: &[u8]) -> IResult<&[u8], Sign> {
        let (input, sign) = opt(one_of("+-"))(input)?;
        let sign = match sign {
            None => Sign::None,
            Some('+') => Sign::Plus,
            Some('-') => Sign::Minus,
            Some(_) => unreachable!("Already checked + or -"),
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

    #[adorn(traceable_parser("integer_with_sign"))]
    fn object_numeric_integer(input: &[u8]) -> IResult<&[u8], Integer> {
        let (input, (sign, digits)) = tuple((parse_sign, digit1))(input)?;
        Ok((
            input,
            Integer {
                sign,
                digits: Cow::Borrowed(digits),
            },
        ))
    }

    #[adorn(traceable_parser("integer_without_sign"))]
    fn integer_without_sign(input: &[u8]) -> IResult<&[u8], Integer> {
        let (input, digits) = digit1(input)?;
        Ok((
            input,
            Integer {
                sign: Sign::None,
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
        println!("Done testing with input: #{}#", input);
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
    #[adorn(traceable_parser("real"))]
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
                    LiteralStringPart::Escaped(part) => {
                        buf.write_all(b"\\").and(buf.write_all(part))
                    }
                }?
            }
            buf.write_all(b")")
        }
    }

    #[adorn(traceable_parser("eol"))]
    fn eol_marker(input: &[u8]) -> IResult<&[u8], &[u8]> {
        alt((tag(b"\r\n"), tag(b"\r"), tag(b"\n")))(input)
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

    #[adorn(traceable_parser("escape"))]
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
    // #[adorn(traceable_parser("literal_string"))]
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
    // #[adorn(traceable_parser("hex_string"))]
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
    #[adorn(traceable_parser("string"))]
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
        NumberSignPrefixed(u8, u8),
    }
    impl BinSerialize for NameObjectPart {
        fn serialize_to(&self, buf: &mut Vec<u8>) -> io::Result<()> {
            match self {
                NameObjectPart::Regular(n) => buf.push(*n),
                NameObjectPart::NumberSignPrefixed(n1, n2) => {
                    buf.push(b'#');
                    buf.push(*n1);
                    buf.push(*n2);
                }
            }
            Ok(())
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
        // // This is the strict version per spec, but I see a dict like "<</Type/Font/Subtype/Type0/BaseFont/ABCDEE+等线,Bold/Encoding/Identity-H/DescendantFonts 8 0 R/ToUnicode 29 0 R>>"
        // is_regular_char(c) && b'!' <= c && c <= b'~'
        is_regular_char(c)
    }

    #[adorn(traceable_parser("name"))]
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
                ret.push(NameObjectPart::NumberSignPrefixed(inp[i + 1], inp[i + 2]));
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
    // Real-life failure
    test_round_trip!(name301: "/AGSWKP#2bHelvetica");
    // "/ABCDEE+等线,Bold"
    test_round_trip_b!(name302: b"/ABCDEE+\xE7\xAD\x89\xE7\xBA\xBF,Bold");

    // ===================
    // 7.3.6 Array Objects
    // ===================
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

    #[adorn(traceable_parser("array_part"))]
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
        let (input, _) = tag(b"[")(input)?;
        let (input, (parts, _)) = many_till(array_object_part, tag(b"]"))(input)?;
        Ok((input, ArrayObject { parts }))
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
        Value(ObjectOrReference<'a>),
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
    // TODO: This is rubbish (does not recognize alternation of key-value). Fix.
    #[adorn(traceable_parser("dict_part"))]
    fn dictionary_part(input: &[u8]) -> IResult<&[u8], DictionaryPart> {
        let (_, first) = take(1usize)(input)?;
        if is_white_space_char(first[0]) || first[0] == b'%' {
            map(whitespace_and_comments, |w| {
                DictionaryPart::Whitespace(Cow::Borrowed(w))
            })(input)
        } else if first == b"/" {
            map(object_name, |name| DictionaryPart::Key(name))(input)
        } else {
            alt((map(object_or_ref, |value| DictionaryPart::Value(value)),))(input)
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
        let (input, _) = tag(b"<<")(input)?;
        let (input, (parts, _)) = many_till(dictionary_part, tag(b">>"))(input)?;
        Ok((input, DictionaryObject { parts }))
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
    // From real life, lightly modified. Note the "/companyName, LLC" as key!
    test_round_trip!(dict202: r"<<
    /Universal PDF(The process that creates this PDF ... United States)
    /Producer(pdfeTeX-1.21a; modified using iText� 5.5.6 �2000-2015 iText Group NV \(AGPL-version\))
    /Creator(TeX)
    /companyName, LLC(http://www.example.com)
    /ModDate(D:20170416015229+05'30')
    /CreationDate(D:20170331194508+02'00')
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

    // ====================
    // 7.3.8 Stream Objects
    // ====================

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

    #[adorn(traceable_parser("whitespace_and_comments"))]
    fn whitespace_and_comments(input: &[u8]) -> IResult<&[u8], &[u8]> {
        let mut i = 0;
        while i < input.len() {
            let c = input[i];
            if is_white_space_char(c) {
                i += 1;
                continue;
            }
            if c == b'%' {
                i += 1;
                while i < input.len() && input[i] != b'\r' && input[i] != b'\n' {
                    i += 1;
                }
                if i == input.len() {
                    break;
                }
                assert!(input[i] == b'\n' || input[i] == b'\r');
                if input[i] == b'\n' {
                    i += 1;
                } else if input[i] == b'\r' {
                    if i + 1 < input.len() && input[i + 1] == b'\n' {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            // Not whitespace or comment any more.
            break;
        }
        // println!(
        //     "Parsed whitespace and comments: {:?} in {:?}",
        //     &input[..i],
        //     backtrace::Backtrace::new()
        // );
        Ok((&input[i..], &input[..i]))
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
        let (input, ws) = whitespace_and_comments(input)?;
        let (_, _) = take(1usize)(ws)?;
        Ok((input, ws))
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

    // =================
    // 7.3.9 Null Object
    // =================
    test_round_trip!(null101: "null");

    // =======================
    // 7.3.10 Indirect Objects
    // =======================
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

    #[cfg(test)]
    // https://stackoverflow.com/a/42067321
    pub fn str_from_u8_nul_utf8(utf8_src: &[u8]) -> Result<&str, std::str::Utf8Error> {
        let nul_range_end = utf8_src
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(utf8_src.len()); // default to length if no `\0` present
        std::str::from_utf8(&utf8_src[0..nul_range_end])
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
        let out = str_from_u8_nul_utf8(&buf).unwrap();
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
    /* #endregion objects... */

    // ==================
    // 7.5 File Structure
    // ==================
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
