# staveloom-cli

Command-line tool for rendering MusicXML/`.mxl` scores to SVG, MIDI, and MP3
— part of [Staveloom](https://github.com/amoriya/staveloom), built on the
[`staveloom-core`](https://crates.io/crates/staveloom-core) rendering engine.

## Install

```bash
cargo install staveloom-cli
```

This installs a `staveloom` binary.

## Usage

```bash
# Basic rendering (generate SVG)
staveloom <file.musicxml> --output out.svg

# Render only specific parts
staveloom <file.musicxml> --parts P1,P2 --output filtered.svg

# Generate MIDI and MP3 audio
staveloom <file.musicxml> --midi out.mid --sf2 path/to/font.sf2 --audio out.mp3

# Extract playback sync metadata (JSON)
staveloom <file.musicxml> --metadata sync_data.json

# List parts
staveloom <file.musicxml> --list-parts

# Layout strategies
staveloom <file.musicxml> --elastic    # duration-based non-linear spacing
staveloom <file.musicxml> --mobile     # dense packing for narrow screens
staveloom <file.musicxml> --horizontal # single long line, no page breaks
staveloom <file.musicxml> --width 800  # page width in px (default: 1200)

# Show license and third-party notices
staveloom --license
```

### MP3 rendering requirements

`--audio` synthesizes via `rustysynth` (a SoundFont/SF2 player) and encodes
with the `lame` crate, which dynamically links the system's `libmp3lame`
(LGPL-2.0, not bundled — install it separately, e.g. `apt install
libmp3lame0`). `--sf2` requires a SoundFont file of your own; none is
bundled with this crate.

## License

Dual-licensed under **MIT OR Apache-2.0**. See
[`LICENSE-MIT`](https://github.com/amoriya/staveloom/blob/master/LICENSE-MIT) /
[`LICENSE-APACHE`](https://github.com/amoriya/staveloom/blob/master/LICENSE-APACHE)
in the repository, or run `staveloom --license`. A review of third-party
licenses is in
[`docs/THIRDPARTY_LICENSE.md`](https://github.com/amoriya/staveloom/blob/master/docs/THIRDPARTY_LICENSE.md).
