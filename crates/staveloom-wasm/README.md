# staveloom-wasm

WASM bindings for the [`staveloom-core`](https://crates.io/crates/staveloom-core)
MusicXML/MIDI rendering engine — part of
[Staveloom](https://github.com/amoriya/staveloom). This is the crate behind
Staveloom's [browser-based web player](https://github.com/amoriya/staveloom),
which does all rendering, MIDI synthesis, and playback client-side with no
server involved.

Most users should use the prebuilt web player rather than this crate
directly, unless you're embedding Staveloom's renderer into your own
JS/TS web app.

## Build

```bash
cargo install wasm-pack
wasm-pack build --target web
```

This produces a `pkg/` directory with `staveloom_wasm.js` (JS glue) and
`staveloom_wasm_bg.wasm`, importable as an ES module.

## JavaScript API

```javascript
import init, {
  list_parts,
  list_instruments,
  list_midi_parts,
  parse_and_render,
  parse_midi_and_render,
} from "./pkg/staveloom_wasm.js";

await init();

const bytes = new Uint8Array(/* .musicxml, .mxl, or .mid file contents */);

// MusicXML/.mxl input:
const parts = list_parts(bytes); // [{ id, name }]
const instruments = list_instruments(bytes); // [{ program, name }]
const result = parse_and_render(
  bytes,
  /* elastic */ false,
  /* mobile */ false,
  /* horizontal */ false,
  /* page_width */ 1200,
  /* filter_parts_csv */ "",
); // { systems, metadata, midi }

// MIDI (.mid) input — same RenderResult shape, routed through a MIDI→Score
// parser instead of the MusicXML parser:
const midiParts = list_midi_parts(midiBytes);
const midiResult = parse_midi_and_render(
  midiBytes,
  false,
  false,
  false,
  1200,
  "",
);
```

`result.systems` is an array of per-system SVG strings (or a single combined
SVG if the score fits on one system); `result.metadata` carries beat/measure
timing data for playback sync; `result.midi` is a regenerated standard MIDI
file (SMF) byte array kept in sync with that metadata.

## License

Dual-licensed under **MIT OR Apache-2.0**. See
[`LICENSE-MIT`](https://github.com/amoriya/staveloom/blob/master/LICENSE-MIT) /
[`LICENSE-APACHE`](https://github.com/amoriya/staveloom/blob/master/LICENSE-APACHE)
in the repository. A review of third-party licenses (fonts, soundfonts,
bundled JS in the web player) is in
[`docs/THIRDPARTY_LICENSE.md`](https://github.com/amoriya/staveloom/blob/master/docs/THIRDPARTY_LICENSE.md).
