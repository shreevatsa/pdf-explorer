Some code that parses a PDF file into objects. There's also some very preliminary WIP code to display the file structure on a web page.

# What is this?

At the lowest level, a PDF file is a sequence of 8-bit bytes. Use `hexdump` or `xxd` or something to see it.

At the next lowest level, a PDF file is a set of **objects**, wrapped in some file structure (header and trailer). This is still too low-level to be practically useful for anything, but the code here only concerns itself with this level.

Specifically, there are 8 types of objects:

-   [x] Boolean Objects
-   [x] Numeric Objects
-   [x] String Objects
-   [x] Name Objects
-   [x] Array Objects
-   [x] Dictionary Objects
-   [x] Stream Objects
-   [x] Null Object

Additionally, we also (need to) parse:

-   [x] Indirect Object definitions (12 0 obj … endobj)
-   [x] Indirect object references (12 0 R)
-   [x] File structure: Header, body, cross-reference table, trailer.

Some notes:

-   It can now round-trip via JSON. That is, if you dump to JSON and read back, you will get the exact same bytes.

    -   This is not as impressive as it sounds, because we could in principle just dump the sequence of bytes into JSON as an array of numbers. However, here we're doing _slightly_ more than that.

-   Assumes the input is valid, e.g. does not check in dict for unique keys, does not check for stream length, etc. In fact, parses the file "forwards", rather than starting with the trailer first.

## Try it out

- Using from Rust code: `src/lib.rs` has a library that parses a PDF file into PDF objects.

- Web interface (WIP, calls the parser but does not display anything much yet):
  - Visit https://shreevatsa.net/pdf-explorer/ or
  - Run `build.sh` and `python3 -m http.server` (or [equivalent](https://gist.github.com/willurd/5720255)), then access http://[::]:8000/

## Similar (better) projects

I haven't yet tried either of these, but they seem to be further along (IIUC they're written in Python and generate HTML):

- https://github.com/desgeeko/pdfsyntax
  - `git clone https://github.com/desgeeko/pdfsyntax && cd pdfsyntax` and then `python3 -m pdfsyntax inspect foo.pdf  > output.html`
- https://github.com/trailofbits/polyfile
  - `pip3 install polyfile` and then `polyfile --html output.html foo.pdf`

## Old notes

Status currently:

- As of 2022-05-01 14:20 (e54b45e): Works for 19426 out of 19492 PDF files on my laptop. So "fails" for 66 files. Looked at each of them. They are all malformed in some way or the other.
