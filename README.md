An example project where Rust code prints the length of an uploaded file.

Run python3 -m http.server (or equivalent: https://gist.github.com/willurd/5720255),
then access http://[::]:8000/

Writing a "pdf file object parser". Starting with parsing individual objects.
There are 8 types of objects:

-   [x] Boolean Objects
-   [x] Numeric Objects
-   [x] String Objects
-   [x] Name Objects
-   [x] Array Objects
-   [ ] Dictionary Objects
-   [ ] Stream Objects
-   [ ] Null Object

We will also need to parse 

-   [ ] Indirect Object definitions (12 0 obj)
-   [ ] Indirect object references (12 0 R), 
-   [ ] File structure: Header, body, cross-reference table, trailer.

