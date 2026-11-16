//! The decode side of the retail movie player (FE 24): the EA MV container (`container`) and VP6 video through a
//! replaceable [`FrameDecoder`]. The VP6 decoder is nihav-vp6 (MIT, vendor/nihav-vp6, two local fixes that make it
//! byte-identical to FFmpeg on all 11 disc movies, research b898 / b921). No rendering, audio or ECS here: the game
//! decodes on a worker thread and uploads the planes (docs/hails-additions/47-movie-player.md).
pub mod audio;
pub mod container;

/// One decoded picture, YUV 4:2:0, top row first: `planes[0]` is luma (`width` x `height`), 1 and 2 are the chroma
/// planes at half size, each tightly packed.
#[derive(Clone, Debug, PartialEq)]
pub struct YuvFrame {
    pub width: usize,
    pub height: usize,
    pub planes: [Vec<u8>; 3],
}

impl YuvFrame {
    pub fn plane_size(&self, plane: usize) -> (usize, usize) {
        if plane == 0 { (self.width, self.height) } else { (self.width.div_ceil(2), self.height.div_ceil(2)) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// The first packet's header could not be read (not VP6 or a cut file).
    Header,
    /// A frame did not decode; the player keeps the last good frame.
    Frame,
}

/// A movie video decoder: packets in, frames out. A mod or platform can put its own decoder behind this.
pub trait FrameDecoder: Send {
    fn decode(&mut self, packet: &[u8]) -> Result<YuvFrame, DecodeError>;
}

/// VP6 (EA flavor: stored bottom-up, flipped here) through nihav.
pub struct Vp6Decoder {
    dec: nihav_duck::codecs::vp6::VP56Decoder,
    br: nihav_duck::codecs::vp6::VP6BR,
    support: nihav_core::codecs::NADecoderSupport,
    ready: bool,
}

impl Default for Vp6Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Vp6Decoder {
    pub fn new() -> Self {
        Self {
            dec: nihav_duck::codecs::vp6::VP56Decoder::new(6, false, true),
            br: nihav_duck::codecs::vp6::VP6BR::new(),
            support: nihav_core::codecs::NADecoderSupport::new(),
            ready: false,
        }
    }

    fn init(&mut self, first: &[u8]) -> Result<(), DecodeError> {
        use nihav_duck::codecs::vp6::VP56Parser;
        let mut bc = nihav_duck::codecs::vpcommon::BoolCoder::new(first).map_err(|_| DecodeError::Header)?;
        let hdr = self.br.parse_header(&mut bc).map_err(|_| DecodeError::Header)?;
        let (w, h) = (hdr.mb_w as usize * 16, hdr.mb_h as usize * 16);
        if w == 0 || h == 0 {
            return Err(DecodeError::Header);
        }
        let info = nihav_codec_support::codecs::NAVideoInfo::new(w, h, true, nihav_codec_support::codecs::YUV420_FORMAT);
        self.dec.init(&mut self.support, info).map_err(|_| DecodeError::Header)?;
        self.ready = true;
        Ok(())
    }
}

impl FrameDecoder for Vp6Decoder {
    fn decode(&mut self, packet: &[u8]) -> Result<YuvFrame, DecodeError> {
        if !self.ready {
            self.init(packet)?;
        }
        let (buf, _) = self.dec.decode_frame(&mut self.support, packet, &mut self.br).map_err(|_| DecodeError::Frame)?;
        let nihav_codec_support::codecs::NABufferType::Video(buf) = buf else { return Err(DecodeError::Frame) };
        let data = buf.get_data();
        let (width, height) = buf.get_dimensions(0);
        let plane = |p: usize| {
            let (pw, ph) = buf.get_dimensions(p);
            let (off, stride) = (buf.get_offset(p), buf.get_stride(p));
            let mut out = Vec::with_capacity(pw * ph);
            for row in (0..ph).rev() {
                out.extend_from_slice(&data[off + row * stride..off + row * stride + pw]);
            }
            out
        };
        Ok(YuvFrame { width, height, planes: [plane(0), plane(1), plane(2)] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_that_is_not_vp6_is_a_header_error() {
        let mut d = Vp6Decoder::new();
        assert_eq!(d.decode(&[]), Err(DecodeError::Header));
    }

    /// Byte-identical to the FFmpeg decode of the user's disc movies (research b898 / b921), when the setup exported
    /// them: SKATE_MOVIE_CHECK=<file.vp6>;<sha256 of the yuv420p frames> runs it.
    #[test]
    fn a_disc_movie_decodes_like_ffmpeg() {
        let Ok(spec) = std::env::var("SKATE_MOVIE_CHECK") else { return };
        let (path, want) = spec.split_once(';').expect("file;sha256");
        let mut r = container::Reader::new(std::fs::File::open(path).expect("movie")).expect("container");
        let mut d = Vp6Decoder::new();
        let mut all = Vec::new();
        while let Some(c) = r.next_chunk().expect("chunk") {
            if let container::Chunk::Video { data, .. } = c {
                let f = d.decode(&data).expect("frame");
                for p in &f.planes {
                    all.extend_from_slice(p);
                }
            }
        }
        assert_eq!(sha256_hex(&all), want.to_ascii_lowercase());
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }
}
