verso src/lib.rs src/bin.rs | recto weave-out weave-in/about.md
pandoc -s -o weave-out/weave-in/about.html weave-out/weave-in/about.md
