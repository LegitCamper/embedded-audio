use heapless::Vec;

use crate::{AudioFile, Channels, PlatformFile, PlatformFileError, SampleFormat};

const MAX_CHUNKS: usize = 25;

#[derive(Debug)]
pub enum Error {
    /// No Riff chunk found
    NoRiffChunkFound,
    /// No Wave chunk found
    NoWaveChunkFound,
    /// No Wave tag found
    NoWaveTagFound,
    /// No Fmt chunk found
    NoFmtChunkFound,
    /// No Data chunk found
    NoDataChunkFound,
    /// Failed to parse fmt chunk
    FmtChunkError,
    /// File contains unsupported Format
    UnsupportedAudioFormat,
    /// Could not parse the chunk based on tag/id
    UnknownChunk,
    /// Unknown audio encoding
    UnknownEncoding,
    /// Unsupported channel count
    UnsupportedChannelCount,
    /// The provided buffer is too small
    ChunkSizeIncorrect,
    /// Exceeded maximum chunks
    ExceededMaxChunks,
    /// Platform File error
    PlatformError(PlatformFileError),
}

/// Wav file parser
pub struct Wav<File: PlatformFile> {
    file: File,
    data_read: usize,
    data_start: usize,
    data_end: usize,
    fmt: Fmt,
}

impl<File: PlatformFile> Wav<File> {
    pub fn new(mut file: File) -> Result<Self, Error> {
        let mut chunks: Vec<Chunk, MAX_CHUNKS> = Vec::new();
        let mut buf = [0_u8; 64];

        // get riff before getting sub chunks
        file.read(&mut buf).map_err(Error::PlatformError)?;
        chunks
            .push(parse_chunk(
                buf[..8].try_into().map_err(|_| Error::ChunkSizeIncorrect)?,
                0,
            ))
            .unwrap();

        parse_chunks(&mut buf, &mut file, &mut chunks, 12)?;

        let fmt_chunk = chunks
            .iter()
            .find(|chunk| chunk.chunk == ChunkTag::Fmt)
            .ok_or(Error::NoFmtChunkFound)?;
        file.seek_from_start(fmt_chunk.start)
            .map_err(Error::PlatformError)?;
        file.read(&mut buf).map_err(Error::PlatformError)?;
        let fmt = parse_fmt(&buf)?;

        // TODO: can look for other chunks in list or info

        let data_chunk = chunks
            .iter()
            .find(|chunk| chunk.chunk == ChunkTag::Data)
            .ok_or(Error::NoDataChunkFound)?;
        file.seek_from_start(data_chunk.start)
            .map_err(Error::PlatformError)?;

        Ok(Self {
            file,
            fmt,
            data_read: 0,
            data_start: data_chunk.start,
            data_end: data_chunk.end,
        })
    }
}

impl<File: PlatformFile> AudioFile<File> for Wav<File> {
    type Error = Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // ensure the only data being read is audio data from data chunk
        let buf = if buf.len() + self.data_read >= self.data_end {
            &mut buf[..self.data_end - self.data_read]
        } else {
            &mut buf[..]
        };

        match self.file.read(buf) {
            Ok(len) => {
                self.data_read += len;
                Ok(len)
            }
            Err(e) => Err(Error::PlatformError(e)),
        }
    }

    fn sample_rate(&self) -> u16 {
        self.fmt.sample_rate
    }

    fn channels(&self) -> Channels {
        self.fmt.channels
    }

    fn sample_format(&self) -> SampleFormat {
        self.fmt.sample_format
    }

    fn try_seek(&mut self, sample_offset: i64) -> Result<(), Self::Error> {
        let byte_offset = sample_offset * self.sample_format().size() as i64;
        self.file
            .seek_from_current(byte_offset)
            .map_err(Error::PlatformError)
    }

    fn is_eof(&self) -> bool {
        self.data_end == self.data_read
    }

    fn played(&self) -> usize {
        self.data_read
    }
}

/// parses the file in the first pass to find out where each chunk is located
fn parse_chunks<File: PlatformFile, const MAX_CHUNKS: usize>(
    buf: &mut [u8],
    file: &mut File,
    chunks: &mut Vec<Chunk, MAX_CHUNKS>,
    file_offset: usize,
) -> Result<(), Error> {
    file.seek_from_start(file_offset)
        .map_err(Error::PlatformError)?;
    let read_len = match file.read(buf) {
        Ok(len) => len,
        Err(PlatformFileError::EOF) => return Ok(()),
        Err(e) => return Err(Error::PlatformError(e)),
    };

    if read_len == 0 {
        return Ok(()); // EOF
    }

    let mut index = 0;

    while index + 8 <= read_len {
        chunks
            .push(parse_chunk(
                &buf[index..index + 8]
                    .try_into()
                    .map_err(|_| Error::ChunkSizeIncorrect)?,
                file_offset + index,
            ))
            .map_err(|_| Error::ExceededMaxChunks)?;

        let last_chunk = chunks.last().unwrap();
        let chunk_len = last_chunk.end - last_chunk.start + 8;

        if index + chunk_len <= read_len {
            index += chunk_len;
        } else {
            return parse_chunks(buf, file, chunks, chunks.last().unwrap().end);
        }
    }
    parse_chunks(buf, file, chunks, file_offset + read_len)
}

fn parse_chunk(bytes: &[u8; 8], index: usize) -> Chunk {
    let tag = ChunkTag::from_bytes(&bytes[..4].try_into().unwrap());
    let mut chunk_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;

    // padding if chunk_len is odd (RIFF word alignment)
    if chunk_len % 2 != 0 {
        chunk_len += 1;
    }

    Chunk {
        chunk: tag,
        start: index + 8,
        end: index + 8 + chunk_len,
    }
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum ChunkTag {
    Riff,
    Rifx, // riff but declaring file as big-endian
    Wave,
    Fmt,
    Data,
    Unknown([u8; 4]),
}

impl ChunkTag {
    fn from_bytes(bytes: &[u8; 4]) -> Self {
        match bytes {
            [b'R', b'I', b'F', b'F'] => Self::Riff,
            [b'R', b'I', b'F', b'X'] => Self::Rifx,
            [b'W', b'A', b'V', b'E'] => Self::Wave,
            [b'd', b'a', b't', b'a'] => Self::Data,
            [b'f', b'm', b't', b' '] => Self::Fmt,
            _ => Self::Unknown(*bytes),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Chunk {
    /// start of chunk data after chunk tag and len
    pub start: usize,
    /// chunk tag/id
    pub chunk: ChunkTag,
    /// end of the chunk
    pub end: usize,
}

struct Fmt {
    audio_format: AudioFormat,
    sample_rate: u16,
    channels: Channels,
    sample_format: SampleFormat,
    extra: Option<ExtraFmtParam>,
}

struct ExtraFmtParam {
    param_size: u16,
    // params: &[]
}

#[derive(PartialEq, Eq)]
enum AudioFormat {
    Pcm,
}

impl AudioFormat {
    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let format = u16::from_le_bytes(bytes.try_into().map_err(|_| Error::ChunkSizeIncorrect)?);
        match format {
            1 => Ok(Self::Pcm),
            _ => Err(Error::UnsupportedAudioFormat),
        }
    }
}

fn parse_fmt(buf: &[u8]) -> Result<Fmt, Error> {
    let format = AudioFormat::from_bytes(&buf[0..2])?;

    let num_channels = u16::from_le_bytes(
        buf[2..4]
            .try_into()
            .map_err(|_| Error::ChunkSizeIncorrect)?,
    );
    let channels = match num_channels {
        1 => Channels::Mono,
        2 => Channels::Stereo,
        _ => return Err(Error::UnsupportedChannelCount),
    };

    let sample_rate = u32::from_le_bytes(
        buf[4..8]
            .try_into()
            .map_err(|_| Error::ChunkSizeIncorrect)?,
    ) as u16;
    let bit_depth = u16::from_le_bytes(
        buf[14..16]
            .try_into()
            .map_err(|_| Error::ChunkSizeIncorrect)?,
    );

    let encoding = match bit_depth {
        8 => SampleFormat::U8,
        16 => SampleFormat::I16,
        24 => SampleFormat::I24,
        _ => return Err(Error::UnknownEncoding),
    };

    Ok(Fmt {
        audio_format: format,
        sample_rate,
        channels,
        sample_format: encoding,
        extra: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{AudioFormat, Wav};
    use crate::{AudioFile, Channels, SampleFormat, TestFile, wav::Error};

    #[test]
    fn parse_fmt() {
        let bytes = [
            0x01, 0x00, // audio format
            0x01, 0x00, // channel count
            0x40, 0x1f, 0x00, 0x00, // sample rate
            0x80, 0x3e, 0x00, 0x00, // byte rate
            0x20, 0x00, // block align
            0x10, 0x00, // bits per sample
        ];

        let fmt = super::parse_fmt(&bytes).unwrap();
        assert!(fmt.audio_format == AudioFormat::Pcm);
        assert!(fmt.sample_rate == 8_000);
        assert!(fmt.sample_format == SampleFormat::I16);
        assert!(fmt.channels == Channels::Mono);
    }

    #[test]
    fn parse_le_16bit_8k_mono() {
        let file = TestFile::from_bytes(&[
            0x52, 0x49, 0x46, 0x46, // RIFF
            0x32, 0x00, 0x00, 0x00, // chunk size
            0x57, 0x41, 0x56, 0x45, // WAVE
            0x66, 0x6d, 0x74, 0x20, // fmt
            0x10, 0x00, 0x00, 0x00, // fmt chunk size
            0x01, 0x00, // audio format
            0x01, 0x00, // channel count
            0x40, 0x1f, 0x00, 0x00, // sample rate
            0x80, 0x3e, 0x00, 0x00, // byte rate
            0x20, 0x00, // block align
            0x10, 0x00, // bits per sample
            0x64, 0x61, 0x74, 0x61, // data
            0x08, 0x00, 0x00, 0x00, // data chunk size
            0x01, 0x00, // sample 1
            0xfe, 0xff, // sample 2
            0x02, 0x00, // sample 3
            0xff, 0xff, // sample 4
        ]);
        let mut wav = Wav::new(file).unwrap();

        assert!(wav.fmt.channels == Channels::Mono);
        assert!(wav.fmt.sample_rate == 8_000);
        assert!(wav.fmt.sample_format == SampleFormat::I16);

        let mut sample = [0_u8; 2]; // size of one sample
        wav.read(&mut sample).unwrap();
        assert!(sample == [0x01, 0x00]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0xfe, 0xff]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0x02, 0x00]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0xff, 0xff]);
    }

    #[test]
    fn parse_le_8bit_8k_stereo() {
        let file = TestFile::from_bytes(&[
            0x52, 0x49, 0x46, 0x46, // RIFF
            0x32, 0x00, 0x00, 0x00, // chunk size
            0x57, 0x41, 0x56, 0x45, // WAVE
            0x66, 0x6d, 0x74, 0x20, // fmt
            0x10, 0x00, 0x00, 0x00, // fmt chunk size
            0x01, 0x00, // audio format
            0x02, 0x00, // channel count
            0x40, 0x1f, 0x00, 0x00, // sample rate
            0x80, 0x3e, 0x00, 0x00, // byte rate
            0x20, 0x00, // block align
            0x08, 0x00, // bits per sample
            0x64, 0x61, 0x74, 0x61, // data
            0x08, 0x00, 0x00, 0x00, // data chunk size
            0x01, 0x00, // sample 1 L+R
            0xfe, 0xff, // sample 2 L+R
            0x02, 0x00, // sample 3 L+R
            0xff, 0xff, // sample 4 L+R
        ]);
        let mut wav = Wav::new(file).unwrap();

        assert!(wav.fmt.channels == Channels::Stereo);
        assert!(wav.fmt.sample_rate == 8_000);
        assert!(wav.fmt.sample_format == SampleFormat::U8);

        let mut sample = [0_u8; 2]; // size of one sample L+R
        wav.read(&mut sample).unwrap();
        assert!(sample == [0x01, 0x00]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0xfe, 0xff]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0x02, 0x00]);
        wav.read(&mut sample).unwrap();
        assert!(sample == [0xff, 0xff]);
    }
}
