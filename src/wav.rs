use crate::{Channels, Encoding, PlatformFile, PlatformFileErrors};

const HEADER_SIZE: usize = 44;
const MAX_CHUNKS: usize = 20;

pub enum Error {
    /// No Riff chunk found
    NoRiffChunkFound,
    /// No Wave chunk found
    NoWaveChunkFound,
    /// No Wave tag found
    NoWaveTagFound,
    /// Failed to parse fmt chunk
    FmtChunkError,
    /// File contains unsupported Format
    UnsupportedFormat,
    /// Chunk tag/id unknown
    UnknownChunkTag,
    /// Could not parse the chunk based on tag/id
    UnknownChunk,
    /// Unknown audio encoding
    UnknownEncoding,
    UnsupportedChannelCount,
    /// The provided buffer is too small
    BufferTooSmall,
    PlatformError(PlatformFileErrors),
}

/// Wav file parser
pub struct Wav<'a, File: PlatformFile> {
    file: &'a File,
    data_read: usize,
    data_start: usize,
    data_end: usize,
    sample_rate: u16,
    channels: Channels,
    encoding: Encoding,
}

impl<'a, File: PlatformFile> Wav<'a, File> {
    pub fn new(file: &'a mut File) -> Result<Self, Error> {
        let mut bytes: [u8; HEADER_SIZE] = [0; HEADER_SIZE];
        let read = file
            .read(&mut bytes)
            .map_err(|_| Error::PlatformError(PlatformFileErrors::FailedRead))?;
        let mut chunks = [None; MAX_CHUNKS];
        parse_chunks(&bytes[..read], &mut chunks)?;

        let fmt_chunk = chunks
            .iter()
            .filter(|chunk| {
                if let Some(chunk) = chunk {
                    chunk.chunk == ChunkTag::Fmt
                } else {
                    false
                }
            })
            .next()
            .unwrap();
        let fmt = parse_fmt(&bytes[fmt_chunk.unwrap().start..fmt_chunk.unwrap().end])?;

        let data_chunk = chunks
            .iter()
            .filter(|chunk| {
                if let Some(chunk) = chunk {
                    chunk.chunk == ChunkTag::Data
                } else {
                    false
                }
            })
            .next()
            .unwrap();

        Ok(Self {
            file,
            sample_rate: fmt.0,
            channels: fmt.1,
            encoding: fmt.2,
            data_read: 0,
            data_start: data_chunk.unwrap().start,
            data_end: data_chunk.unwrap().end,
        })
    }
}

pub fn parse_chunks<'a, const MAX_CHUNKS: usize>(
    bytes: &'a [u8],
    chunks: &mut [Option<Chunk>; MAX_CHUNKS],
) -> Result<(), Error> {
    let riff = Chunk::from_bytes(bytes)?;

    if riff.chunk != ChunkTag::Riff {
        return Err(Error::NoRiffChunkFound);
    }

    if ChunkTag::from_bytes(
        bytes[8..8 + 4]
            .try_into()
            .map_err(|_| Error::BufferTooSmall)?,
    )
    .map_err(|_| Error::NoWaveTagFound)?
        != ChunkTag::Wave
    {
        return Err(Error::NoWaveTagFound);
    }

    // skip parsed bytes
    let mut index = 12;
    let mut num_chunks = 0;

    while index < bytes.len() {
        let chunk = &bytes[index..];
        let chunk_info = Chunk::from_bytes(chunk)?;

        // Chunks should always have an even number of bytes,
        // if it is odd there is an empty padding byte at the end
        let chunk_length = chunk_info.end - chunk_info.start;
        let padding_byte = (chunk_length & 1) * 8;

        index += 8 + chunk_length + padding_byte;

        chunks[num_chunks] = Some(chunk_info);
        num_chunks += 1;
        if num_chunks >= MAX_CHUNKS {
            break;
        }
    }

    Ok(())
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum ChunkTag {
    Riff,
    Wave,
    Fmt,
    Data,
}

impl ChunkTag {
    fn from_bytes(buf: &[u8; 4]) -> Result<Self, Error> {
        match buf {
            [b'R', b'I', b'F', b'F'] => Ok(Self::Riff),
            [b'W', b'A', b'V', b'E'] => Ok(Self::Wave),
            [b'd', b'a', b't', b'a'] => Ok(Self::Data),
            [b'f', b'm', b't', b' '] => Ok(Self::Fmt),
            _ => Err(Error::UnknownChunk),
        }
    }
}

#[derive(Copy, Clone)]
pub struct Chunk {
    /// start of chunk data after chunk tag and len
    pub start: usize,
    /// chunk tag/id
    pub chunk: ChunkTag,
    /// end of the chunk
    pub end: usize,
}

impl Chunk {
    fn from_bytes(buf: &[u8]) -> Result<Self, Error> {
        let chunk =
            ChunkTag::from_bytes(&buf[0..4].try_into().map_err(|_| Error::BufferTooSmall)?)?;
        let size = u32::from_le_bytes(buf[4..8].try_into().map_err(|_| Error::BufferTooSmall)?);
        let start = 8 + 12;

        Ok(Self {
            start,
            chunk,
            end: start + size as usize,
        })
    }
}

fn parse_fmt(buf: &[u8]) -> Result<(u16, Channels, Encoding), Error> {
    let format = u16::from_le_bytes(buf[0..2].try_into().map_err(|_| Error::FmtChunkError)?);

    if format != 1 {
        return Err(Error::UnsupportedFormat);
    }

    let num_channels = u16::from_le_bytes(buf[2..4].try_into().map_err(|_| Error::FmtChunkError)?);
    let channels = match num_channels {
        1 => Channels::Mono,
        2 => Channels::Stereo,
        _ => return Err(Error::UnsupportedChannelCount),
    };

    let sample_rate =
        u32::from_le_bytes(buf[4..8].try_into().map_err(|_| Error::FmtChunkError)?) as u16;
    let bit_depth = u16::from_le_bytes(buf[14..16].try_into().map_err(|_| Error::FmtChunkError)?);

    let encoding = match bit_depth {
        8 => Encoding::U8Bit,
        16 => Encoding::S16Bit,
        24 => Encoding::S24Bit,
        _ => return Err(Error::UnknownEncoding),
    };

    Ok((sample_rate, channels, encoding))
}
