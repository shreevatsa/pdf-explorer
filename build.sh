wasm-pack build --target web --release
sed -i".bak" "s/async function __wbg_init/export async function __wbg_init/" pkg/pdf_explorer.js
