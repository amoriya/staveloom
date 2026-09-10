import init, { parse_and_render } from "../../../../web/pkg/staveloom_wasm.js";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

async function main() {
  const args = process.argv.slice(2);
  if (args.length < 4) {
    console.error(
      "Usage: node wasm_render.mjs <input.xml> <out.svg> <out_meta.json> <out.mid>",
    );
    process.exit(1);
  }

  const [inputPath, outSvg, outMeta, outMidi] = args;

  // Load WASM
  const wasmPath = path.resolve(
    __dirname,
    "../../../../web/pkg/staveloom_wasm_bg.wasm",
  );
  const wasmBytes = fs.readFileSync(wasmPath);
  await init(wasmBytes);

  // Read input
  const xmlBytes = new Uint8Array(fs.readFileSync(inputPath));

  // Render (page_width = 1200, elastic = false, horizontal = false)
  const result = parse_and_render(xmlBytes, false, false, 1200, "");

  // Write outputs
  // Note: in Phase 1, we return a single system SVG containing the entire SVG
  fs.writeFileSync(outSvg, result.systems[0].svg_content);
  fs.writeFileSync(outMeta, JSON.stringify(result.metadata, null, 2));
  fs.writeFileSync(outMidi, Buffer.from(result.midi));

  console.log(`Successfully rendered ${inputPath} via WASM`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
