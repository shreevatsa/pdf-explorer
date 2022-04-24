wasm-pack build --target web
sed -i".bak" "s/async function init/export async function init/" pkg/hello_wasm.js
