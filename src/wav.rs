use crate::{AudioFile, Channels, PlatformFile, SampleFormat};

const HEADER_SIZE: usize = 44;
const MAX_CHUNKS: usize = 5;

#[derive(Debug)]
pub enum Error<PlatformError> {
    /// No Riff chunk found
    NoRiffChunkFound,
    /// No Wave chunk found
    NoWaveChunkFound,
    /// No Wave tag found
    NoWaveTagFound,
    /// Failed to parse fmt chunk
    FmtChunkError,
    /// File contains unsupported Format
    UnsupportedAudioFormat,
    /// Chunk tag/id unknown
    UnknownChunkTag,
    /// Could not parse the chunk based on tag/id
    UnknownChunk,
    /// Unknown audio encoding
    UnknownEncoding,
    UnsupportedChannelCount,
    /// The provided buffer is too small
    BufferSizeIncorrect,
    PlatformError(PlatformError),
}

/// Wav file parser
pub struct Wav<File: PlatformFile> {
    file: File,
    data_read: usize,
    data_start: usize,
    data_end: usize,
    fmt: Fmt,
}

impl<'a, File: PlatformFile> Wav<File> {
    pub fn new(mut file: File) -> Result<Self, Error<File::Error>> {
        let mut bytes: [u8; HEADER_SIZE] = [0; HEADER_SIZE];
        let read = file.read(&mut bytes).map_err(Error::PlatformError)?;
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
        let fmt = parse_fmt(&bytes[fmt_chunk.unwrap().start + 8..fmt_chunk.unwrap().end])?;

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

        file.seek_from_start(data_chunk.unwrap().start + 8)
            .map_err(Error::PlatformError);

        Ok(Self {
            file,
            fmt,
            data_read: 0,
            data_start: data_chunk.unwrap().start,
            data_end: data_chunk.unwrap().end,
        })
    }
}

impl<'a, File: PlatformFile> AudioFile<File> for Wav<File> {
    type Error = Error<File::Error>;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error<File::Error>> {
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
}

pub fn parse_chunks<'a, PlatformError, const MAX_CHUNKS: usize>(
    bytes: &'a [u8],
    chunks: &mut [Option<Chunk>; MAX_CHUNKS],
) -> Result<(), Error<PlatformError>> {
    let riff = parse_chunk(bytes, 0)?;

    if riff.chunk != ChunkTag::Riff && riff.chunk != ChunkTag::Rifx {
        return Err(Error::NoRiffChunkFound);
    }

    if ChunkTag::from_bytes(
        bytes[8..12]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
    )
    .map_err(|_: Error<PlatformError>| Error::NoWaveTagFound)?
        != ChunkTag::Wave
    {
        return Err(Error::NoWaveTagFound);
    }

    // skip to subchunks
    let mut index = 12;
    let mut num_chunks = 0;

    while index < bytes.len() {
        let chunk = parse_chunk(bytes, index)?;

        // align end to even byte
        index = chunk.end + ((chunk.end & 1) * 8);

        chunks[num_chunks] = Some(chunk);
        num_chunks += 1;
        if num_chunks >= MAX_CHUNKS {
            break;
        }
    }

    Ok(())
}

fn parse_chunk<PlatformError>(bytes: &[u8], start: usize) -> Result<Chunk, Error<PlatformError>> {
    let tag = ChunkTag::from_bytes(
        &bytes[start..start + 4]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
    )?;
    let size = u32::from_le_bytes(
        bytes[start + 4..start + 8]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
    ) + 8; // +8 is size of chunk tag and chumk size

    Ok(Chunk {
        start,
        chunk: tag,
        end: start + size as usize,
    })
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum ChunkTag {
    Riff,
    Rifx, // riff but declaring file as big-endian
    Wave,
    Fmt,
    Data,
}

impl ChunkTag {
    fn from_bytes<PlatformError>(bytes: &[u8; 4]) -> Result<Self, Error<PlatformError>> {
        match bytes {
            [b'R', b'I', b'F', b'F'] => Ok(Self::Riff),
            [b'R', b'I', b'F', b'X'] => Ok(Self::Rifx),
            [b'W', b'A', b'V', b'E'] => Ok(Self::Wave),
            [b'd', b'a', b't', b'a'] => Ok(Self::Data),
            [b'f', b'm', b't', b' '] => Ok(Self::Fmt),
            _ => Err(Error::UnknownChunkTag),
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
    PCM,
}

impl AudioFormat {
    fn from_bytes<PlatformError>(bytes: &[u8]) -> Result<Self, Error<PlatformError>> {
        let format = u16::from_le_bytes(bytes.try_into().map_err(|_| Error::BufferSizeIncorrect)?);
        match format {
            1 => Ok(Self::PCM),
            _ => Err(Error::UnsupportedAudioFormat),
        }
    }
}

fn parse_fmt<PlatformError>(buf: &[u8]) -> Result<Fmt, Error<PlatformError>> {
    let format = AudioFormat::from_bytes(&buf[0..2])?;

    let num_channels = u16::from_le_bytes(
        buf[2..4]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
    );
    let channels = match num_channels {
        1 => Channels::Mono,
        2 => Channels::Stereo,
        _ => return Err(Error::UnsupportedChannelCount),
    };

    let sample_rate = u32::from_le_bytes(
        buf[4..8]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
    ) as u16;
    let bit_depth = u16::from_le_bytes(
        buf[14..16]
            .try_into()
            .map_err(|_| Error::BufferSizeIncorrect)?,
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
    use crate::{AudioFile, Channels, SampleFormat, TestFile, TestFileError, wav::Error};

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

        let fmt = super::parse_fmt::<TestFileError>(&bytes).unwrap();
        assert!(fmt.audio_format == AudioFormat::PCM);
        assert!(fmt.sample_rate == 8_000);
        assert!(fmt.sample_format == SampleFormat::I16);
        assert!(fmt.channels == Channels::Mono);
    }

    #[test]
    fn parse_le_16bit_8k_mono() {
        let mut file = TestFile::from_bytes(&[
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
        let mut file = TestFile::from_bytes(&[
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
