verso src/lib.rs src/bin.rs | (cd docs-src && recto ../docs about.md)
pandoc -s -o docs/about.html docs/about.md
