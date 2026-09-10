pub mod builder;
mod event;
pub mod pitch;
pub mod quantizer;

pub use event::{KeySigChange, ParsedMidi, PartInfo, RawNote, TempoChange, TimeSigChange};
pub use pitch::{choose_clef, drum_notehead, drum_position, midi_to_pitch};
pub use quantizer::{
    decompose_duration, detect_grid, is_human_midi, needs_tolerance_quantization, quantize_note,
    ticks_to_note_value, ticks_to_note_value_tolerant,
};

/// Errors produced by [`MidiParser`].
#[derive(Debug, thiserror::Error)]
pub enum MidiParseError {
    #[error("Invalid MIDI format: {0}")]
    InvalidFormat(String),

    #[error("MIDI Format {0} is not supported (only Format 0 and Format 1 are accepted)")]
    UnsupportedFormat(u16),

    #[error("MIDI decode error: {0}")]
    MidlyError(String),

    #[error("File is empty or contains no note events")]
    EmptyFile,
}

/// Parses a raw MIDI binary into a [`crate::models::Score`] ready for rendering.
pub struct MidiParser;

impl MidiParser {
    /// Parse `bytes` and return the raw event data.
    ///
    /// Supports MIDI Format 0 (single track) and Format 1 (multi-track).
    /// Format 2 returns [`MidiParseError::UnsupportedFormat`].
    pub fn parse_events(bytes: &[u8]) -> Result<ParsedMidi, MidiParseError> {
        event::SmfParser::parse(bytes)
    }

    /// Parse `bytes` and build a complete [`crate::models::Score`].
    ///
    /// Combines event extraction (Phase 1), quantization (Phase 2),
    /// pitch conversion (Phase 3), and score assembly (Phase 4).
    pub fn parse(bytes: &[u8]) -> Result<crate::models::Score, MidiParseError> {
        let parsed = event::SmfParser::parse(bytes)?;
        Ok(builder::build(&parsed))
    }
}
