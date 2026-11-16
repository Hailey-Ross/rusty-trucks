//! The movies' audio: per language one `SD<lang>` block per video frame, EA-XA v2 ADPCM, 6 channels, 48 kHz
//! (research b924). A block (after its tag and size) is: u32 big-endian sample count, one u32 big-endian offset per
//! channel, then each channel's frames at `4 + 4 * channels + offset`. EA-XA v2 frames carry 28 samples: a 15-byte
//! ADPCM frame (coefficient / shift byte, 14 bytes of nibbles, high nibble first) or a 61-byte raw frame (`0xEE`, two
//! big-endian history words, 28 big-endian samples). The history runs on across blocks (v2 streams have no per-block
//! history header). Format reference: vgmstream `ea_xa_decoder.c` / `blocked_ea_schl.c` (decoder written here from it).

/// vgmstream's EA_XA_TABLE: coefficient 1 at `k`, coefficient 2 at `k + 4` (k = the high nibble).
const COEF: [i32; 20] = [0, 240, 460, 392, 0, 0, -208, -220, 0, 1, 3, 4, 7, 8, 10, 11, 0, -1, -3, -4];
const FRAME_SAMPLES: usize = 28;

/// One channel's decoder state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct XaChannel {
    h1: i32,
    h2: i32,
}

impl XaChannel {
    /// Decodes one frame at the start of `data` into `out` (up to 28 samples); returns the frame's size in bytes.
    fn frame(&mut self, data: &[u8], out: &mut Vec<i16>, samples: usize) -> Option<usize> {
        let info = *data.first()?;
        if info == 0xEE {
            let need = 1 + 4 + FRAME_SAMPLES * 2;
            let d = data.get(..need)?;
            let be = |o: usize| i16::from_be_bytes([d[o], d[o + 1]]);
            self.h1 = i32::from(be(1));
            self.h2 = i32::from(be(3));
            out.extend((0..samples).map(|i| be(5 + 2 * i)));
            return Some(need);
        }
        let d = data.get(..15)?;
        let k = usize::from(info >> 4);
        let (c1, c2) = (COEF[k], COEF[k + 4]);
        let shift = u32::from(info & 0x0F) + 8;
        for i in 0..samples {
            let byte = d[1 + i / 2];
            let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
            let mut s = (i32::from(nibble) << 28) >> shift;
            s = (s + c1 * self.h1 + c2 * self.h2) >> 8;
            let s = s.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
            out.push(s as i16);
            self.h2 = self.h1;
            self.h1 = s;
        }
        Some(15)
    }
}

/// Decodes one `SD` block body (after its 8-byte head) for `state.len()` channels; returns the samples interleaved
/// per frame (`channels` values per sample), or `None` for a cut block.
pub fn decode_block(body: &[u8], state: &mut [XaChannel]) -> Option<Vec<i16>> {
    let channels = state.len();
    let be32 = |o: usize| body.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let samples = be32(0)?;
    let base = 4 + 4 * channels;
    let mut per: Vec<Vec<i16>> = Vec::with_capacity(channels);
    for (c, ch) in state.iter_mut().enumerate() {
        let mut at = base + be32(4 + 4 * c)?;
        let mut out = Vec::with_capacity(samples);
        while out.len() < samples {
            let n = (samples - out.len()).min(FRAME_SAMPLES);
            at += ch.frame(body.get(at..)?, &mut out, n)?;
        }
        per.push(out);
    }
    let mut mixed = Vec::with_capacity(samples * channels);
    for i in 0..samples {
        mixed.extend(per.iter().map(|p| p[i]));
    }
    Some(mixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(channels: &[Vec<u8>], samples: u32) -> Vec<u8> {
        let mut b = samples.to_be_bytes().to_vec();
        let mut off = 0u32;
        for c in channels {
            b.extend_from_slice(&off.to_be_bytes());
            off += c.len() as u32;
        }
        for c in channels {
            b.extend_from_slice(c);
        }
        b
    }

    #[test]
    fn an_adpcm_frame_scales_its_nibbles_and_predicts_from_history() {
        // Filter 0 (no prediction), shift 12 + 8: nibble 1 -> (1 << 28) >> 20 >> 8 = 1; nibble 0xF (-1) -> -1.
        let mut f = vec![0x0C];
        f.extend([0x1F; 14]);
        let mut st = [XaChannel::default()];
        let out = decode_block(&block(&[f], 28), &mut st).unwrap();
        assert_eq!(&out[..4], &[1, -1, 1, -1]);
        assert_eq!(out.len(), 28);
        // Filter 1 (240 / 256 of the last sample) carries the history into the next block.
        let mut g = vec![0x1C];
        g.extend([0x00; 14]);
        let next = decode_block(&block(&[g], 2), &mut st).unwrap();
        assert_eq!(next, vec![(-240) >> 8, ((((-240) >> 8) * 240) >> 8)]);
    }

    #[test]
    fn a_raw_frame_is_big_endian_pcm_and_sets_the_history() {
        let mut f = vec![0xEE];
        f.extend(100i16.to_be_bytes());
        f.extend(50i16.to_be_bytes());
        for i in 0..28i16 {
            f.extend((i * 1000 - 9000).to_be_bytes());
        }
        let mut st = [XaChannel::default(); 2];
        let mut g = vec![0x0C];
        g.extend([0x10; 14]);
        let out = decode_block(&block(&[f, g], 3), &mut st).unwrap();
        assert_eq!(out, vec![-9000, 1, -8000, 0, -7000, 1], "interleaved: channel 0 raw, channel 1 ADPCM");
        assert_eq!(st[0], XaChannel { h1: 100, h2: 50 });
    }

    #[test]
    fn a_cut_block_is_refused() {
        let mut st = [XaChannel::default()];
        assert_eq!(decode_block(&block(&[vec![0x0C, 1, 2]], 28), &mut st), None);
        assert_eq!(decode_block(&[0, 0], &mut st), None);
    }
}
