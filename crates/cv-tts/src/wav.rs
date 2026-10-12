//! Reads the header of the WAV bytes OneCore synthesis returns (ADR 0012). Pure logic, no Windows
//! types: it finds the sample format and where the PCM data is, and refuses anything that is not
//! 16-bit integer PCM, which is all the WASAPI player is written for.

use std::ops::Range;

/// The format and data of a PCM WAV file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavPcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Where the sample data is in the file's bytes.
    pub data: Range<usize>,
}

impl WavPcm {
    /// Bytes in one frame (one sample for every channel).
    pub fn frame_bytes(&self) -> usize {
        self.channels as usize * (self.bits_per_sample as usize / 8)
    }
}

const WAVE_FORMAT_PCM: u16 = 1;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Parses `bytes` as a RIFF/WAVE file with 16-bit PCM samples. A data chunk that claims more than
/// the file holds (a streamed file) is cut to what is there, and a trailing partial frame is
/// dropped.
pub fn parse_wav(bytes: &[u8]) -> Result<WavPcm, String> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(bytes, at + 4).ok_or("truncated chunk header")? as usize;
        let body = at + 8;
        if id == b"fmt " {
            let tag = u16_at(bytes, body).ok_or("truncated fmt chunk")?;
            let channels = u16_at(bytes, body + 2).ok_or("truncated fmt chunk")?;
            let rate = u32_at(bytes, body + 4).ok_or("truncated fmt chunk")?;
            let bits = u16_at(bytes, body + 14).ok_or("truncated fmt chunk")?;
            format = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (tag, channels, sample_rate, bits_per_sample) = format.ok_or("data before fmt")?;
            if tag != WAVE_FORMAT_PCM {
                return Err(format!("format tag {tag} is not integer PCM"));
            }
            if bits_per_sample != 16 {
                return Err(format!("{bits_per_sample}-bit samples; only 16-bit is supported"));
            }
            if channels == 0 || sample_rate == 0 {
                return Err("zero channels or sample rate".into());
            }
            let end = body.saturating_add(size).min(bytes.len());
            let mut wav = WavPcm { channels, sample_rate, bits_per_sample, data: body..end };
            let whole = (wav.data.len() / wav.frame_bytes()) * wav.frame_bytes();
            wav.data.end = wav.data.start + whole;
            return Ok(wav);
        }
        // Chunks are padded to an even size.
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, extra: &[(&[u8; 4], Vec<u8>)], data: &[u8]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
        out.extend(b"fmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(tag.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        out.extend((channels * bits / 8).to_le_bytes());
        out.extend(bits.to_le_bytes());
        for (id, body) in extra {
            out.extend(*id);
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
            if body.len() % 2 == 1 {
                out.push(0);
            }
        }
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn a_plain_16_bit_file_is_read() {
        let bytes = wav(1, 1, 22050, 16, &[], &[0; 8]);
        let w = parse_wav(&bytes).unwrap();
        assert_eq!((w.channels, w.sample_rate, w.bits_per_sample), (1, 22050, 16));
        assert_eq!(w.data.len(), 8);
        assert_eq!(w.frame_bytes(), 2);
        assert_eq!(&bytes[w.data.clone()], &[0u8; 8]);
    }

    #[test]
    fn other_chunks_before_data_are_skipped_with_padding() {
        let bytes = wav(1, 2, 48000, 16, &[(b"LIST", vec![1, 2, 3])], &[1, 2, 3, 4]);
        let w = parse_wav(&bytes).unwrap();
        assert_eq!(&bytes[w.data], &[1, 2, 3, 4]);
    }

    #[test]
    fn a_data_size_larger_than_the_file_is_cut_to_the_file() {
        let mut bytes = wav(1, 1, 22050, 16, &[], &[0; 6]);
        let at = bytes.len() - 6 - 4;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_wav(&bytes).unwrap().data.len(), 6);
    }

    #[test]
    fn a_partial_frame_at_the_end_is_dropped() {
        let bytes = wav(1, 2, 22050, 16, &[], &[0; 7]);
        assert_eq!(parse_wav(&bytes).unwrap().data.len(), 4);
    }

    #[test]
    fn formats_that_are_not_16_bit_integer_pcm_are_refused() {
        assert!(parse_wav(&wav(3, 1, 22050, 32, &[], &[0; 4])).unwrap_err().contains("not integer PCM"));
        assert!(parse_wav(&wav(1, 1, 22050, 8, &[], &[0; 4])).unwrap_err().contains("8-bit"));
        assert!(parse_wav(&wav(1, 0, 22050, 16, &[], &[0; 4])).is_err());
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(parse_wav(b"").is_err());
        assert!(parse_wav(b"RIFF").is_err());
        assert!(parse_wav(b"RIFF\0\0\0\0WAVEfmt ").is_err());
        assert!(parse_wav(b"RIFF\0\0\0\0WAVEjunk\xff\xff\xff\xff").is_err());
        let no_data = wav(1, 1, 22050, 16, &[], &[]);
        assert_eq!(parse_wav(&no_data).unwrap().data.len(), 0);
        assert!(parse_wav(&no_data[..no_data.len() - 8]).unwrap_err().contains("no data"));
    }
}
