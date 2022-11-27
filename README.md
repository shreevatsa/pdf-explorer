Some code that parses a PDF file into objects, and optionally (wip) displays them on a web page.

# What is this?

At the lowest level, a PDF file is a sequence of 8-bit bytes.

At the next lowest level (and almost just as useless), is a PDF file is a collection of **objects** (surrounded by a header and trailer). The code here only concerns itself with this level.

## Try it out

Using from Rust code: `src/lib.rs` has a library that parses a PDF file into PDF objects.

Web interface (WIP, calls the parser but does not display anything much yet):

- Run `build.sh` and `python3 -m http.server` (or equivalent: https://gist.github.com/willurd/5720255), then access http://[::]:8000/, or
- Visit https://shreevatsa.net/pdf-explorer/ (the trailing slash is important, unfortunately)

## Old notes

Writing a "pdf file object parser". Starting with parsing individual objects.
There are 8 types of objects:

-   [x] Boolean Objects
-   [x] Numeric Objects
-   [x] String Objects
-   [x] Name Objects
-   [x] Array Objects
-   [x] Dictionary Objects
-   [x] Stream Objects
-   [x] Null Object

We will also need to parse 

-   [x] Indirect Object definitions (12 0 obj)
-   [x] Indirect object references (12 0 R), 
-   [x] File structure: Header, body, cross-reference table, trailer.

Some notes:

-   It can now round-trip (objects, not yet an entire PDF file) via JSON. That is, if you dump to JSON and read back, you will get the exact same bytes.

    -   This is not as big a deal as it sounds, because we could in principle dump the sequence of bytes into JSON as an array of numbers. However, here we're doing _slightly_ more than that.

-   Assumes the input is valid, e.g. does not check in dict for unique keys, does not check for stream length, etc.

Status currently:

- Out of 19560 PDF files I have, this works correctly for 8724 of them.

- As of 2022-04-30 (970471e): Works for 19262 out of 19560 files. So fails for 298 (not all of which are actually PDF files).

- As of 2022-05-01 (child of 970471e): Works for 19430 out of 19562 files. So fails for 132 files.

- As of 2022-05-01 (after deleting some dupes): Works for 19382 out of 19493 files. So fails for 111 files.

- As of 2022-05-01 11:52: Works for 19420 out of 19493 files. So fails for 73 files.

- As of 2022-05-01 14:20 (e54b45e): Works for 19426 out of 19492 files. So "fails" for 66 files. Looked at each of them. They are all malformed in some way or the other.
