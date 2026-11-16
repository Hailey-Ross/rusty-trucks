//! The EA movie container of the disc's `.vp6` files (research b924): chunks of a 4-byte tag and a u32 little-endian
//! size (header included). `MVhd` first, then per language `SH<lang>` / `SC<lang>`, then per video frame one
//! `MV0K` (key) or `MV0F` followed by one `SD<lang>` audio block per language. The reader streams: it keeps one chunk
//! in memory at a time.
use std::io::{Read, Seek, SeekFrom};

/// `MVhd`: `VP60`, width, height, frame count, largest frame, rate / scale (299699 / 10000 = 29.97 fps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub width: u16,
    pub height: u16,
    pub frames: u32,
    pub largest_frame: u32,
    pub rate: u32,
    pub scale: u32,
}

impl Header {
    pub fn fps(&self) -> f64 {
        if self.scale == 0 { 0.0 } else { f64::from(self.rate) / f64::from(self.scale) }
    }

    fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 24 || &body[..4] != b"VP60" {
            return None;
        }
        let u16le = |o: usize| u16::from_le_bytes([body[o], body[o + 1]]);
        let u32le = |o: usize| u32::from_le_bytes([body[o], body[o + 1], body[o + 2], body[o + 3]]);
        Some(Self { width: u16le(4), height: u16le(6), frames: u32le(8), largest_frame: u32le(12), rate: u32le(16), scale: u32le(20) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Chunk {
    /// A video frame (`MV0K` = key frame).
    Video { key: bool, data: Vec<u8> },
    /// An audio block of the language tag (`SD` + two characters, e.g. "EN").
    Audio { language: [u8; 2], data: Vec<u8> },
    /// Any other chunk (`SH??`, `SC??`, unknown tags), skipped by the player.
    Other { tag: [u8; 4] },
}

pub struct Reader<R> {
    inner: R,
    pub header: Header,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file does not start with an `MVhd` chunk holding a `VP60` header.
    NotAMovie,
    /// A chunk's size runs past the end of the file or is below its own header.
    BadChunk { at: u64 },
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl<R: Read + Seek> Reader<R> {
    pub fn new(mut inner: R) -> Result<Self, Error> {
        let (tag, size) = read_head(&mut inner)?.ok_or(Error::NotAMovie)?;
        if &tag != b"MVhd" || size < 8 {
            return Err(Error::NotAMovie);
        }
        let mut body = vec![0; size as usize - 8];
        inner.read_exact(&mut body)?;
        let header = Header::parse(&body).ok_or(Error::NotAMovie)?;
        Ok(Self { inner, header })
    }

    /// The next chunk, `None` at the end of the file.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>, Error> {
        let at = self.inner.stream_position()?;
        let Some((tag, size)) = read_head(&mut self.inner)? else { return Ok(None) };
        if size < 8 {
            return Err(Error::BadChunk { at });
        }
        let len = size as usize - 8;
        let chunk = match &tag {
            b"MV0K" | b"MV0F" => Chunk::Video { key: &tag == b"MV0K", data: self.body(len, at)? },
            [b'S', b'D', a, b] => Chunk::Audio { language: [*a, *b], data: self.body(len, at)? },
            _ => {
                self.inner.seek(SeekFrom::Current(len as i64))?;
                Chunk::Other { tag }
            }
        };
        Ok(Some(chunk))
    }

    /// Back to the first chunk after the header (a looping movie).
    pub fn rewind(&mut self) -> Result<(), Error> {
        self.inner.seek(SeekFrom::Start(0))?;
        let (_, size) = read_head(&mut self.inner)?.ok_or(Error::NotAMovie)?;
        self.inner.seek(SeekFrom::Start(u64::from(size)))?;
        Ok(())
    }

    fn body(&mut self, len: usize, at: u64) -> Result<Vec<u8>, Error> {
        let mut data = vec![0; len];
        self.inner.read_exact(&mut data).map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { Error::BadChunk { at } } else { Error::Io(e) })?;
        Ok(data)
    }
}

/// A chunk head, `None` at a clean end of file.
fn read_head<R: Read>(r: &mut R) -> Result<Option<([u8; 4], u32)>, Error> {
    let mut head = [0u8; 8];
    let mut got = 0;
    while got < 8 {
        let n = r.read(&mut head[got..])?;
        if n == 0 {
            return if got == 0 { Ok(None) } else { Err(Error::BadChunk { at: 0 }) };
        }
        got += n;
    }
    Ok(Some(([head[0], head[1], head[2], head[3]], u32::from_le_bytes([head[4], head[5], head[6], head[7]]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = tag.to_vec();
        v.extend_from_slice(&(body.len() as u32 + 8).to_le_bytes());
        v.extend_from_slice(body);
        v
    }

    fn header() -> Vec<u8> {
        let mut h = b"VP60".to_vec();
        for v in [1280u16.to_le_bytes(), 720u16.to_le_bytes()] { h.extend_from_slice(&v); }
        for v in [3u32, 4096, 299_699, 10_000] { h.extend_from_slice(&v.to_le_bytes()); }
        h
    }

    #[test]
    fn the_header_and_chunks_are_read_in_order() {
        let mut file = chunk(b"MVhd", &header());
        file.extend(chunk(b"SHEN", &[0; 0x30]));
        file.extend(chunk(b"MV0K", &[1, 2, 3]));
        file.extend(chunk(b"SDEN", &[9; 5]));
        file.extend(chunk(b"MV0F", &[4]));
        let mut r = Reader::new(std::io::Cursor::new(file)).unwrap();
        assert_eq!((r.header.width, r.header.height, r.header.frames), (1280, 720, 3));
        assert!((r.header.fps() - 29.9699).abs() < 1e-4);
        assert_eq!(r.next_chunk().unwrap(), Some(Chunk::Other { tag: *b"SHEN" }));
        assert_eq!(r.next_chunk().unwrap(), Some(Chunk::Video { key: true, data: vec![1, 2, 3] }));
        assert_eq!(r.next_chunk().unwrap(), Some(Chunk::Audio { language: *b"EN", data: vec![9; 5] }));
        assert_eq!(r.next_chunk().unwrap(), Some(Chunk::Video { key: false, data: vec![4] }));
        assert_eq!(r.next_chunk().unwrap(), None);
        r.rewind().unwrap();
        assert_eq!(r.next_chunk().unwrap(), Some(Chunk::Other { tag: *b"SHEN" }));
    }

    #[test]
    fn a_file_without_a_vp60_header_or_with_a_cut_chunk_is_refused() {
        assert!(matches!(Reader::new(std::io::Cursor::new(chunk(b"RIFF", &[0; 24]))), Err(Error::NotAMovie)));
        let mut file = chunk(b"MVhd", &header());
        let mut cut = chunk(b"MV0K", &[1; 16]);
        cut.truncate(12);
        file.extend(cut);
        let mut r = Reader::new(std::io::Cursor::new(file)).unwrap();
        assert!(matches!(r.next_chunk(), Err(Error::BadChunk { .. })));
    }
}
