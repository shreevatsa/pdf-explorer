wasm-pack build --target web --release
sed -i".bak" "s/async function init/export async function init/" pkg/pdf_explorer.js
