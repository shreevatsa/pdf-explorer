wasm-pack build --target web --release
verso src/lib.rs src/bin.rs | recto weave-out weave-in/about.md
