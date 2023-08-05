import { init, handle_file } from "./pkg/pdf_explorer.js";

console.log("Worker: Hello.");
await init();
console.log("Worker: Done WASM init.");

onmessage = async function (e) {
    console.log('Worker: Message received from main script:', e);
    const ret = handle_file(e.data);
    console.log('Worker: Posting message back to main script.');
    postMessage(ret);
}
