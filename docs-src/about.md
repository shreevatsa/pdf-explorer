---
title: What's in a PDF file?
---

# Background and plan

At the lowest level, a PDF file is a sequence of 8-bit bytes. There are several tools like `hexdump` and `xxd` that one can use to look at the "raw" bytes in a file, though this is hardly useful.

At the next lowest level, a PDF file is a set of **objects**, wrapped in some file structure (header and trailer). This is what we shall look at here (though note that this is barely any more useful to look at than the raw bytes).

Specifically, there are 8 kinds of objects:

-   [boolean objects](#boolean-objects)
-   [numeric objects](#numeric-objects)
-   string objects
-   name objects
-   array objects
-   dictionary objects
-   stream objects
-   the null object

Let's look at each of them in more detail below, before looking at the rest of the file structure of PDF files (defining and referring to objects, and the header, cross-reference table, and trailer).

To make all this concrete, we will write some code to parse a PDF file: it will read a PDF file and turn it into data structures representing these objects, then write out these internal data structures into file bytes. (Parsing/deserialization, then serialization.) If we've done everything correctly, the output bytes must be the same as the input bytes. (This is not very useful, as I already warned you.)

The code is written in Rust, using [`nom`](https://github.com/rust-bakery/nom) as the library for parsing. I'm new to them too, so I'll explain those parts too.

Aside: If you wish, you can follow along in the PDF spec, specifically the "Syntax" chapter. (This is Chapter 3 in the [pleasant Adobe version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf), and Chapter 7 in the [sterile ISO version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf) / [PDF 2.0](https://pdfa.org/sponsored-standards/).)

# Boolean objects

These are simply the keywords `true` and `false`.

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

# Numeric objects

There are two kinds of numeric objects: integers, and reals.

## Integer

An integer is "one or more decimal digits optionally preceded by a sign". A typical PDF parser wouldn't have to care about a leading `+` sign or leading `0`s, but we need to, as we're trying to write one that can round-trip.

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

## Real

```rs
@@numeric/real
```

# The rest of the library

The rest of the lib file (`@?lib.file`), which is:

```rs
@@lib/1
```

and

```rs
@@lib
```

# The binary wrapper

There is a binary wrapper in `@?bin.file` to exercise this:

```rs
@@bin
```
