# staveloom-core

Core MusicXML/MIDI parsing, layout, and SVG rendering engine for
[Staveloom](https://github.com/amoriya/staveloom) — a publication-quality
music notation renderer written in Rust. This crate is a pure library (no
I/O beyond reading the input you hand it): it never touches the filesystem,
network, or a GUI, which makes it usable from a CLI, a WASM binding, or any
other host.

This crate powers [`staveloom-cli`](https://crates.io/crates/staveloom-cli)
(a command-line tool) and [`staveloom-wasm`](https://crates.io/crates/staveloom-wasm)
(browser/WASM bindings, used by the [live web player](https://github.com/amoriya/staveloom)).
Most users should reach for one of those two rather than depending on this
crate directly, unless you're embedding notation rendering into your own
Rust application.

## What it does

- **Parse**: MusicXML/`.mxl` bytes → a `Score` data model (`parser`/`models`)
- **Render**: `Score` → an SVG string, with elastic/compact/mobile spacing
  strategies and collision-free vertical stacking of dynamics, lyrics,
  chord symbols, etc. (`renderer`)
- **Resolve playback order**: repeats, volta endings, D.S./D.C., Segno/Coda
  → a flat sequence of measure indices (`timeline`)
- **Generate MIDI**: `Score` + timeline → a standard MIDI file (SMF, via
  `midly`), expanding trills/mordents/arpeggios/tremolos into real note
  events (`midi_engine`)
- **Parse MIDI back into notation**: a raw `.mid` file → a `Score`, with
  tick quantization and human-performance detection (`midi_parser`)
- **Auto-beaming**: assigns beam groups to notes that don't already specify
  one (`auto_beam`)

## Example

```rust
use staveloom_core::parser::load_and_parse;
use staveloom_core::renderer::Renderer;
use staveloom_core::timeline::TimelineSolver;
use staveloom_core::midi_engine::MidiEngine;
use std::path::Path;

let score = load_and_parse(Path::new("score.musicxml"))?;

let renderer = Renderer::default(); // or Renderer::mobile() / apply_mobile_preset()
let (svg, metadata) = renderer.render_with_metadata(&score);

let timeline = TimelineSolver::solve(&score);
let smf = MidiEngine::generate_smf(&score, &timeline);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`parse_in_memory(bytes: &[u8])` is available alongside `load_and_parse` for
callers that don't have a filesystem path (e.g. a WASM host).

## License

Dual-licensed under **MIT OR Apache-2.0**. See
[`LICENSE-MIT`](https://github.com/amoriya/staveloom/blob/master/LICENSE-MIT) /
[`LICENSE-APACHE`](https://github.com/amoriya/staveloom/blob/master/LICENSE-APACHE)
in the repository. A review of third-party licenses (fonts, soundfonts,
bundled JS in the web player) is in
[`docs/THIRDPARTY_LICENSE.md`](https://github.com/amoriya/staveloom/blob/master/docs/THIRDPARTY_LICENSE.md).
