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

To read a PDF file, we need to be able to parse all these kinds of objects, and also, additionally:

-   [x] Indirect Object definitions (like `12 0 obj … endobj`)
-   [x] Indirect object references (like `12 0 R`)
-   [x] File structure: Header, body, cross-reference table, trailer.

This project parses (most) PDF files, into the above structures. It does nothing further in terms of interpreting the objects in any useful way.

It is also (sort of, WIP) a "literate program" to some extent: read [here](https://shreevatsa.net/pdf-explorer/docs/about.html).

Some notes:

-   It can now round-trip via JSON. That is, if you dump to JSON and read back, you will get the exact same bytes.

    -   This is not as impressive as it sounds, because we could in principle just dump the sequence of bytes into JSON as an array of numbers. However, here we're doing _slightly_ more than that.

-   It assumes the input is valid, e.g. does not check in dict for unique keys, does not check for stream length, etc. In fact, parses the file "forwards", rather than starting with the trailer first.

## Try it out

- Using from Rust code: `src/lib.rs` has a library that parses a PDF file into PDF objects.

- Web interface (WIP, calls the parser but does not display anything much yet):
  - Visit https://shreevatsa.net/pdf-explorer/ (last working version: https://638396b5cb23920d58f8adf4--fastidious-ganache-d72698.netlify.app/) or
  - Run `build.sh` and `python3 -m http.server` (or [equivalent](https://gist.github.com/willurd/5720255)), then access http://[::]:8000/

## Similar (better) projects

I haven't yet tried either of these, but they seem to be further along (IIUC they're written in Python and generate HTML):

- https://github.com/desgeeko/pdfsyntax
  - `git clone https://github.com/desgeeko/pdfsyntax && cd pdfsyntax` and then `python3 -m pdfsyntax inspect foo.pdf  > output.html`
- https://github.com/trailofbits/polyfile
  - `pip3 install polyfile` and then `polyfile --html output.html foo.pdf`

There's also a Java app:

- https://github.com/itext/i7j-rups (https://itextpdf.com/products/rups)
  - Download the release and then `java -jar ~/Downloads/itext-rups-7.2.5.jar` etc.

 Others:

- https://www.reportmill.com/snaptea/PDFViewer/ = https://www.reportmill.com/snaptea/PDFViewer/pviewer.html (drag PDF onto it)
- https://pdf.hyzyla.dev/
- https://sourceforge.net/projects/pdfinspector/ (an "example" of https://superficial.sourceforge.net/)
- https://www.o2sol.com/pdfxplorer/overview.htm

## Old notes

Status currently:

- As of 2022-05-01 14:20 (e54b45e): Works for 19426 out of 19492 PDF files on my laptop. So "fails" for 66 files. Looked at each of them. They are all malformed in some way or the other.
