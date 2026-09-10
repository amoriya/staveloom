use lame::Lame;
use midly::Smf;
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tempfile::NamedTempFile;

pub struct AudioRenderer {
    soundfont: Arc<SoundFont>,
    sample_rate: u32,
}

impl AudioRenderer {
    pub fn new(sf2_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(sf2_path)?;
        let soundfont = Arc::new(SoundFont::new(&mut file)?);
        Ok(Self {
            soundfont,
            sample_rate: 44100,
        })
    }

    pub fn render_to_mp3(
        &self,
        smf: &Smf,
        output_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Save Smf to a temporary MIDI file
        let tmp_midi = NamedTempFile::new()?;
        smf.save(tmp_midi.path())?;

        // 2. Load the file for rustysynth
        let mut midi_file_data = File::open(tmp_midi.path())?;
        let midi_file = Arc::new(MidiFile::new(&mut midi_file_data)?);

        // 3. Setup Synth and Sequencer
        let settings = SynthesizerSettings::new(self.sample_rate as i32);
        let synthesizer = Synthesizer::new(&self.soundfont, &settings)?;
        let mut sequencer = MidiFileSequencer::new(synthesizer);
        sequencer.play(&midi_file, false);

        // 4. Init LAME
        let mut lame = Lame::new().ok_or("Failed to initialize LAME")?;
        lame.set_sample_rate(self.sample_rate)
            .map_err(|e| format!("LAME error: {:?}", e))?;
        lame.set_channels(2)
            .map_err(|e| format!("LAME error: {:?}", e))?;
        lame.set_quality(2)
            .map_err(|e| format!("LAME error: {:?}", e))?;
        lame.init_params()
            .map_err(|e| format!("LAME error: {:?}", e))?;

        let mut mp3_file = File::create(output_path)?;

        // 5. Render chunks
        let mut left_buf = vec![0.0f32; 1024];
        let mut right_buf = vec![0.0f32; 1024];

        while !sequencer.end_of_sequence() {
            sequencer.render(&mut left_buf, &mut right_buf);

            let l_i16: Vec<i16> = left_buf
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect();
            let r_i16: Vec<i16> = right_buf
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect();

            let mut mp3_buffer = vec![0u8; 1024 * 5 / 4 + 7200];
            let size = lame
                .encode(&l_i16, &r_i16, &mut mp3_buffer)
                .map_err(|e| format!("LAME error: {:?}", e))?;
            mp3_file.write_all(&mp3_buffer[..size])?;
        }

        let mut final_buffer = vec![0u8; 7200];
        let size = lame
            .encode(&[], &[], &mut final_buffer)
            .map_err(|e| format!("LAME error: {:?}", e))?;
        mp3_file.write_all(&final_buffer[..size])?;

        Ok(())
    }
}
