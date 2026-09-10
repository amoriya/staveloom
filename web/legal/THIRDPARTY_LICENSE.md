# Third-Party Licenses

This document lists the third-party open-source software, fonts, and audio
assets bundled with or depended on by this project (`staveloom-core`, `staveloom-cli`,
`staveloom-wasm`, and the `web/` player), and reviews whether any of them impose
obligations beyond what's already satisfied.

This file does not declare a license for staveloom's own source code. staveloom itself
is dual-licensed under MIT OR Apache-2.0, per `LICENSE-MIT`/`LICENSE-APACHE`.
It only covers what's incorporated from elsewhere.

## Summary of findings

- **No GPL or otherwise strong-copyleft code is compiled into any binary.**
  All Rust crate dependencies are MIT, Apache-2.0, or public-domain-equivalent
  (Unlicense), used under permissive terms.
- **One LGPL component, used only optionally at build/link time:**
  `staveloom-cli`'s `--audio` (MP3 export) feature dynamically links the system's
  `libmp3lame` via the `lame` crate. `libmp3lame` itself is LGPL-2.0, not the
  MIT-licensed `lame` Rust crate that wraps it. See [`lame` /
  `libmp3lame`](#lame-rust-crate--systemlibmp3lame) below for what this
  means for redistributing built binaries.
- **Two bundled web assets were missing their required license notices** —
  fixed as part of this review by adding the upstream license text alongside
  each asset (see below); no functional code changed.
- Everything else (fonts, soundfonts, vendored JS) is permissively licensed
  (MIT / Apache-2.0 / SIL OFL 1.1) and compatible with commercial and
  open-source redistribution.

## Rust dependencies (`staveloom-core`, `staveloom-cli`, `staveloom-wasm`)

### Direct dependencies

| Crate | License | Used by | Notes |
|---|---|---|---|
| [`midly`](https://crates.io/crates/midly) | Unlicense | core, cli | MIDI file read/write |
| [`roxmltree`](https://crates.io/crates/roxmltree) | MIT OR Apache-2.0 | core | MusicXML parsing |
| [`serde`](https://crates.io/crates/serde) / `serde_json` / `serde-wasm-bindgen` | MIT OR Apache-2.0 (serde-wasm-bindgen: MIT) | all | (de)serialization |
| [`svg`](https://crates.io/crates/svg) | MIT OR Apache-2.0 | core | SVG document building |
| [`thiserror`](https://crates.io/crates/thiserror) | MIT OR Apache-2.0 | core | error types |
| [`walkdir`](https://crates.io/crates/walkdir) | Unlicense/MIT | core | test/sample directory traversal |
| [`zip`](https://crates.io/crates/zip) (deflate only) | MIT | core | `.mxl` archive reading |
| [`lame`](https://crates.io/crates/lame) | MIT (crate) — see note | cli | MP3 encoding bindings |
| [`rustysynth`](https://crates.io/crates/rustysynth) | MIT | cli | SoundFont (.sf2) synthesis for `--audio`/`--midi` rendering |
| [`tempfile`](https://crates.io/crates/tempfile) | MIT OR Apache-2.0 | cli | scratch files during audio render |
| [`wasm-bindgen`](https://crates.io/crates/wasm-bindgen) | MIT OR Apache-2.0 | wasm | Rust↔JS bindings |

### Transitive dependencies

Everything pulled in transitively (via `Cargo.lock`) is MIT, Apache-2.0, or
Unlicense, generally dual/triple-licensed so any one permissive option can be
selected — standard for the Rust ecosystem:

`adler2`, `arbitrary`, `bitflags`, `bumpalo`, `cfg-if`, `crc32fast`,
`crossbeam-deque`, `crossbeam-epoch`, `crossbeam-utils`, `derive_arbitrary`,
`displaydoc`, `either`, `equivalent`, `errno`, `fastrand`, `flate2`,
`futures-core`, `futures-task`, `futures-util`, `getrandom`, `hashbrown`,
`indexmap`, `itoa`, `js-sys`, `libc`, `linux-raw-sys`, `log`, `memchr`,
`miniz_oxide`, `once_cell`, `pin-project-lite`, `proc-macro2`, `quote`,
`r-efi`, `rayon`, `rayon-core`, `rustix`, `rustversion`, `same-file`,
`serde_core`, `serde_derive`, `simd-adler32`, `slab`, `syn`,
`thiserror-impl`, `unicode-ident`, `walkdir`, `wasip2`,
`wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared`,
`winapi-util`, `windows-link`, `windows-sys`, `wit-bindgen`, `zmij`,
`zopfli` (Apache-2.0).

Two are worth calling out individually:

- **`r-efi`** additionally offers `LGPL-2.1-or-later` as one of three
  license options (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`). Since it's an
  *OR*, MIT/Apache-2.0 can be selected instead — no LGPL obligation is
  actually incurred. (This crate is a UEFI-target dependency pulled in only
  for non-Linux build targets; it isn't reachable in this project's actual
  build targets, but is listed here for completeness.)
- **`unicode-ident`** is `(MIT OR Apache-2.0) AND Unicode-3.0` — the
  Unicode-3.0 term is an additional, separately-permissive attribution
  license covering the Unicode character-property tables it embeds, not a
  restriction.

### `lame` (Rust crate) / system `libmp3lame`

`staveloom-cli`'s optional `--audio <file.mp3>` flag links against the system's
`libmp3lame` shared library through the `lame` crate (MIT-licensed bindings
with no build script — it expects `libmp3lame` to already be installed and
resolves it via the linker's default dynamic-linking behavior, i.e.
`#[link(name = "mp3lame")]`).

`libmp3lame` itself is **LGPL-2.0**, not MIT. The `lame` crate's own MIT
license only covers its thin Rust wrapper code, not the C library it calls
into.

**Why this is fine as currently used:** `libmp3lame` is dynamically linked
and expected to be provided by the host system (e.g. `apt install
libmp3lame0`), not statically embedded or redistributed inside the `staveloom`
binary. LGPL-2.0 permits this kind of dynamic linking without imposing
LGPL terms on `staveloom-cli` itself.

**What to watch for:** if `staveloom-cli` binaries are ever distributed
pre-built (rather than built from source against whatever `libmp3lame` the
end user has installed), the distributor must still satisfy LGPL-2.0 for
`libmp3lame`'s own object code — in practice this means either (a)
continuing to dynamically link against a separately-installed
`libmp3lame` (already the case), or (b) if statically linking or bundling
`libmp3lame`'s binary in the future, also shipping its source (or a
written offer for it) and permitting users to relink a modified version.
Do not switch this crate to a `static` link kind without re-checking this.

## Web player (`web/`)

The web frontend has no npm runtime dependencies — everything it uses is
either hand-written, generated by `wasm-bindgen`/`wasm-pack` from this
project's own Rust code (`web/pkg/`), or vendored directly as static files:

| Asset | License | Notes |
|---|---|---|
| `web/lib/spessasynth_lib.min.js`, `web/lib/spessasynth_processor.min.js` ([SpessaSynth](https://github.com/spessasus/SpessaSynth)) | Apache-2.0 | SoundFont/MIDI playback engine used by the in-browser player. Unmodified, pre-built minified files as published upstream. License text added at `web/lib/LICENSE-spessasynth.txt` as part of this review (was missing). |
| `web/fonts/Bravura-subset.woff2` ([Bravura](https://github.com/steinbergmedia/bravura), the SMuFL reference font) | SIL Open Font License 1.1 | Subset (212 glyphs) of the original font, embedded for notation rendering. OFL requires the license text accompany any redistributed copy (including subsets); added at `web/fonts/OFL.txt` as part of this review (was missing). "Bravura" is a Reserved Font Name — this subset is not renamed or presented as a different font, which OFL requires. |

## Audio assets (`web/soundfonts/`)

| Asset | License | Notes |
|---|---|---|
| `web/soundfonts/instruments/*.sf2` (129 files) | MIT | Per-instrument split of [FluidR3 GM](https://musical-artifacts.com/artifacts/738) (Frank Wen), produced via `scripts/split_soundfont.py`. The original combined `FluidR3 GM.sf2` this was split from is not bundled in the repo (only the per-instrument output is); instrument display names were relabeled for the UI, sample data is unmodified. |

MIT only requires the copyright and permission notice be retained "in all
copies or substantial portions of the Software" — for a binary sample-data
asset like an SF2 file (rather than source text you can put a header in),
recording that requirement here satisfies it.

## If you add a new dependency

- Rust: prefer MIT/Apache-2.0/BSD-family crates. If a crate's `Cargo.lock`
  entry doesn't show one of those (or Unlicense), check it before merging.
- Anything that wraps or links a native/system library (like `lame` does)
  needs its *actual* native library's license checked separately from the
  Rust binding crate's own license — they're often different.
- Any new bundled font, audio sample library, or vendored JS/CSS file needs
  its upstream license file copied alongside it here, the same way this
  review added `web/fonts/OFL.txt` and `web/lib/LICENSE-spessasynth.txt`.
