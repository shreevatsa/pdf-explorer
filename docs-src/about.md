---
title: What's in a PDF file?
---

# Background and plan

At the lowest level, a PDF file is a sequence of 8-bit bytes. There are several tools like `hexdump` and `xxd` that one can use to look at the "raw" bytes in a file, though this is hardly useful.

At the next lowest level, a PDF file is a set of **objects**, wrapped in some file structure (header and trailer). This is what we shall look at here (though note that this is barely any more useful to look at than the raw bytes).

Specifically, there are 8 kinds of objects:

-   boolean objects
-   numeric objects
-   string objects
-   name objects
-   array objects
-   dictionary objects
-   stream objects
-   the null object

Let's look at each of them in more detail below, before looking at the rest of the file structure of PDF files (defining and referring to objects, and the header, cross-reference table, and trailer).

To make all this concrete, we will write some code to parse a PDF file: it will read a PDF file and turn it into data structures representing these objects, then write out these internal data structures into file bytes. (Parsing/deserialization, then serialization.) If we've done everything correctly, the output bytes must be the same as the input bytes. (This is not very useful, as I already warned you.)

The code is written in Rust, using [`nom`](https://github.com/rust-bakery/nom) as the library for parsing. I'm new to them too, so I'll explain those parts too.

Aside: All this closely follows the PDF spec, specifically the "Syntax" chapter. (Chapter 3 in the [pleasant Adobe version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf), Chapter 7 in the [sterile ISO version of PDF 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf) / [PDF 2.0](https://pdfa.org/sponsored-standards/).)

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
