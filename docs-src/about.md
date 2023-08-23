---
title: What's in a PDF file?
header-includes: |
  <style>
  .aboutCode {
    background-color: Gainsboro;
  }
  body {
    /* The default Pandoc 36em is too narrow for the code lines to fit. */
    max-width: 45em;
  }
  </style>
---


# Background and plan

At the lowest level, a PDF file is a sequence of 8-bit bytes. There are several tools like `hexdump` and `xxd` that one can use to look at the "raw" bytes in a file, though this is hardly useful.

At the next lowest level, a PDF file is a set of **objects**, wrapped in some file structure (header and trailer). This is what we shall look at here (though note that this is barely any more useful to look at than the raw bytes).

Specifically, there are 8 kinds of objects:

- [Background and plan](#background-and-plan)
- [Boolean objects](#boolean-objects)
- [Numeric objects](#numeric-objects)
  - [Integer](#integer)
  - [Real](#real)
  - [Putting them together](#putting-them-together)
- [String objects](#string-objects)
  - [Literal strings](#literal-strings)
  - [Hexadecimal strings](#hexadecimal-strings)
  - [Putting them together](#putting-them-together-1)
- [Name objects](#name-objects)
- [Array objects](#array-objects)
- [Dictionary objects](#dictionary-objects)
- [Stream objects](#stream-objects)
- [Null object](#null-object)
- [An object](#an-object)
- [Refering to an object](#refering-to-an-object)
- [Defining an object](#defining-an-object)
- [The body](#the-body)
- [Cross-ref table](#cross-ref-table)
- [Trailer](#trailer)
- [(Body, crossref, trailer)](#body-crossref-trailer)
- [The overall PDF file](#the-overall-pdf-file)
- [Testing round trips](#testing-round-trips)
- [Tracing](#tracing)
- [All the above is in a module](#all-the-above-is-in-a-module)
- [WASM](#wasm)
- [For the binary wrapper](#for-the-binary-wrapper)

Let's look at each of them in more detail below, before looking at the rest of the file structure of PDF files (defining and referring to objects, and the header, cross-reference table, and trailer).

:::{.aboutCode}
To make all this concrete, we will write some code to parse a PDF file: it will read a PDF file and turn it into data structures representing these objects, then write out these internal data structures into file bytes. (Parsing/deserialization, then serialization.) If we've done everything correctly, the output bytes must be the same as the input bytes. (This is not very useful, as I already warned you.)

The code is written in Rust, using [`nom`](https://github.com/rust-bakery/nom) as the library for parsing. I'm new to them too, so I'll explain those parts too. (Paragraphs styled like these two are explaining the code; you can ignore them if you don't care about them code.)
:::

Aside: If you wish, you can follow along in the PDF spec, specifically the "Syntax" chapter. (This is Chapter 3 in the [pleasant Adobe version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf), and Chapter 7 in the [sterile ISO version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf) / [PDF 2.0](https://pdfa.org/sponsored-standards/).)

# Boolean objects

These are simply the keywords `true` and `false`.

:::{.aboutCode}
It is good for us that the first type of object is so simple, because it gives us an opportunity to explain the code conventions here.

We (I) represent a boolean object in Rust as:

```rs
@@bool/enum
```

This declares a `BooleanObject` as a Rust `enum` ([doc](https://doc.rust-lang.org/std/keyword.enum.html), [book](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html), [reference](https://doc.rust-lang.org/reference/items/enumerations.html)), and the `derive` attribute line automatically generates some functions on this object. (Above, `Serialize` is [`serde::Serialize`](https://docs.rs/serde/1.0.183/serde/trait.Serialize.html) which generates a `serialize` function, and `Deserialize` is [`serde::Deserialize`](https://docs.rs/serde/1.0.183/serde/trait.Deserialize.html) which generates a `deserialize` function, and `Debug` makes it possible to print an instance of this `Enum` with `{:?}`.)

As mentioned earlier, we'll eventually want to write out these data structures to a file. For this, we declare a trait called `BinSerialize`:

```rs
@@BinSerialize
```

and implement it for each object. For example, for `BooleanObject`s, we simply write out the bytes `true` or `false` to the buffer:

```rs
@@bool/BinSerialize
```

The actual code for parsing:

```rs
@@bool/Parse
```

Understanding this requires understanding [`nom`'s conventions](https://tfpk.github.io/nominomicon/chapter_1.html): the function `object_boolean`, which takes an `&[u8]` (slice of bytes) and returns an `IResult<&[u8], BooleanObject>` (denoting either an error, or the "remaining" slice and the parsed `BooleanObject`), is a **parser** in nom's notation. We could write this parser by hand, but in this case we use Nom's ["combinators"](https://github.com/rust-bakery/nom/blob/main/doc/choosing_a_combinator.md):

- `tag("true")` is a parser that parses the string "true" from the start of the input given to it. Similarly `tag("false")`.
- `tag("true").map(|_| BooleanObject::True)` or `map(tag("true"), |_| BooleanObject::True)` is a parser that parses the string "true", and turns it into a `BooleanObject`. (Here, `map` is a combinator that turns one parser into another that parses the same input but returns a different result.)
-  `alt` is a combinator that will "Try a list of parsers and return the result of the first successful one".

Finally we have a bunch of tests:

```rs
@@bool/Tests
```

The `test_round_trip` macro will be explained later, but basically it generates a test that the passed argument ("true" or "false" in this case) round-trips cleanly through our parser: when parsed and written back to a string, the result is identical to the original string.
:::

# Numeric objects

There are two kinds of numeric objects: integers, and reals.

## Integer

An integer is "one or more decimal digits optionally preceded by a sign".

:::{.aboutCode}
A typical PDF parser wouldn't have to care about a leading `+` sign or leading `0`s, but we need to, as we're trying to write one that can round-trip.

First the sign:

```rs
@@numeric/integer/sign
```

Here, `one_of` is a combinator that (returns a parser that) matches one of the characters, and `opt` makes a parser optional. I had originally written it more verbosely as:

```rs
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
```

As for the integer itself, we store it as:

```rs
@@numeric/integer/type
```

The new complication here is that we have to care about Rust concepts of lifetimes (`'a`) and borrowing (`Cow<'a, [u8]`). The reasoning is this:

- Unlike the earlier cases, where, say a  `BooleanObject::True` or a `Sign::Minus` correspond always to the same sequence of bytes (`true` or `-` respectively) and take constant space, when we get to objects like strings and streams (covered later), we'd like to store the actual sequence of bytes that occur in the file. The sequence of digits in an integer is also similar. 
- We could store it as a `vec<u8>` (the object itself would own the sequence of bytes), but instead of cloning (making a copy of) the bytes, it turns out to be faster (especially for strings and streams) to borrow from (so, just keep a reference to) the input bytes.
- We could store it as a borrowed slice `&'a [u8]`, which is fine for the parse path, but when we're serializing to a JSON string with `serde::Serialize`, we'd like cloning to happen automatically, as the string we return should be self-contained without any references.
- It turns out that `serde` does this automatically if we use the [`Cow`](https://doc.rust-lang.org/std/borrow/enum.Cow.html) type, which stands for clone-on-write and holds either a borrowed or an owned value. So we're using this type (introduced in [this commit](https://github.com/shreevatsa/pdf-explorer/commit/484fcf5764da7cecfc78916ead3a89a5c5d227cf)) just for this feature, without ever using the clone-on-write functionality.

The rest of the code for serializing and parsing an integer is fairly straightforward. We introduce an `integer_without_sign` function, to reuse the `Integer` type to store some nonnegative integers that we'll encounter while parsing other objects.

```rs
@@numeric/integer
```
:::

## Real

A real number is similar, except that it contains a decimal point.

:::{.aboutCode}
To round-trip, we store the digits before and after the decimal point, instead of just storing a decimal value.

```rs
@@numeric/real
```
:::

## Putting them together

A numeric object is either an Integer or a Real.

:::{.aboutCode}
(In practice, wherever a real number is expected, we can also simply write an Integer, e.g. write "4" instead of "4.0", but in our code we model this as a NumericObject being expected, in such cases.)

```rs
@@numeric
```

Above, the decorator `#[adorn(traceable_parser("numeric"))]` will be explained later: it is something that makes it easier to see debug output.
:::

# String objects

A string object is a sequence of zero or more bytes. (Depending on the context, there may be some convention for interpreting those bytes, e.g. UTF-8, but the string itself is simply the sequence of bytes.) It can be written in two ways: as a literal string, or as a hexadecimal string.

## Literal strings

A literal string contains the string's bytes between parentheses. For example, it can look like this:

```
(This is a literal string. It can
contain newlines, escapes \t, line \
continuations, characters like !@#$%^&*<>+;][} other
than \\ and parentheses \( \), but balanced
ones ((())() like this ((())()())) are ok,
and octal \101 escapes.)
```

:::{.aboutCode}
A typical PDF parser wouldn't have to care about the difference between

```
(This is a \
string)
```

and

```
(This is a string)
```

Similarly, it wouldn't have to care about whether the PDF file writes `\101` (octal for 65) or the byte `A`. So in a typical PDF parser, the internal representation of a string could simply be a sequence of bytes.

But because we want something that will round-trip cleanly, we need to preserve such details: it turns out that we just need to keep track of each escaped part (usually a single character, but could be octal digits or empty) separately.

```rs
@@string/literal/repr
```

Some tests, to illustrate the kinds of strings we want to be able to parse:

```rs
@@string/literal/tests
```

With this representation, parsing a string literal is mostly just a matter of starting with `(` and going until `)` while keeping track of balanced parentheses and special handling of whatever comes after a backslash:

```rs
@@string/literal/rest
```
:::

## Hexadecimal strings

These are even more straightforward: a hexadecimal string looks like `<901fA>` which means the three bytes `90`, `1F`, and `A0`.

:::{.aboutCode}
In our code (as we're writing a round-trip parser), we just store all the bytes between `<` and `>` (whitespace included), and don't do any interpreting of the hexadecimal numbers.

```rs
@@string/hexadecimal
```
:::

## Putting them together

A string object in a PDF file is either a literal string or a hexadecimal string.

:::{.aboutCode}
```rs
@@string
```
:::

# Name objects

A name, in a PDF file, is written like "/XYZ" -- it is something like what is called a "symbol" in some other languages: "an atomic symbol uniquely defined by a sequence of any characters (8-bit values) except null (character code 0)".

The only catch is that `#20` represents the byte 0x20 (ASCII space), etc. -- this is to be used for all whitespace and delimiters (and per the spec for all characters outside the printable ASCII range `!` to `~`, but in practice I see PDFs ignoring this rule and putting UTF-8 bytes directly in the name).

:::{.aboutCode}
We represent it as:

```rs
@@name/repr
```

so parsing is simply looking byte-by-byte, until encountering the end of the name, treating `#` specially:

```rs
@@name
```
:::

# Array objects

An array is a simply sequence of objects (or indirect object references, to be described later), separated by whitespace, between `[` and `]`. It can contain not only the boolean, numeric, string and name objects described so far, but also array objects, dictionary objects, stream objects, and object references.

Above, "whitespace" includes comments: a comment starts with `%` and goes to the end of the line, and is treated as a single whitespace.

:::{.aboutCode}
We represent the "parts" of an `ArrayObject` to include both the actual `ObjectOrReference`s (to be defined later) that are the array elements, and the whitespace/comments between them.

```rs
@@array/repr
```

As we'll need to parse whitespace and comments, this is a good time to introduce a parser for them, which we'll use heavily later.

```rs
@@comments
```

With this, parsing an array is straightforward, using the `object_or_ref` parser that will be defined later.

```rs
@@array/parse
```
:::

# Dictionary objects

“Dictionary objects are the main building blocks of a PDF document.” They look like `<</key value /key2 value2 ... >>` where the keys are all name objects, and the values are either objects or references to them.

:::{.aboutCode}
```rs
@@dict
```
:::

# Stream objects

:::{.aboutCode}
```rs
@@stream
```
:::

# Null object

:::{.aboutCode}
```rs
@@null
```
:::

# An object

It's not always straightforward, e.g. here is an object we're expected to parse:

```
14 0 obj
<<

endstream
endobj
```

:::{.aboutCode}
```rs
@@object
```
:::

# Refering to an object 

(indirect object reference)

:::{.aboutCode}
```rs
@@indirect_object_reference
```
:::

# Defining an object

:::{.aboutCode}
```rs
@@indirect_object_definition
```
:::

# The body

sequence of object definitions

:::{.aboutCode}
```rs
@@body_part
```

# Cross-ref table

Cross-ref (TODO: Split this further):
```rs
@@cross_ref
```

# Trailer

Trailer:
```rs
@@trailer
```

# (Body, crossref, trailer)

```rs
@@body_crossref_trailer
```
:::

# The overall PDF file

:::{.aboutCode}
```rs
@@pdf_file
```
:::

:::{.aboutCode}
# Testing round trips

```rs
@@TestRoundTrip
```

# Tracing

```rs
@@tracing
```

# All the above is in a module 

```rs
@@mod_header
```

# WASM

```rs
@@wasm
```

# For the binary wrapper

Some more code:

```rs
@@file_parse_and_back
```

There is a binary wrapper in `@?bin.file` to exercise this:

```rs
@@bin
```
:::
