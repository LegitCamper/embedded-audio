#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]

#[cfg(feature = "embedded-sdmmc")]
use embedded_sdmmc::{BlockDevice, File, TimeSource};
#[cfg(feature = "std")]
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub mod wav;

/// File getters for accessing audio data across all supported containers/formats
pub trait AudioFile<File: PlatformFile> {
    type Error;

    /// read audio samples from file
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    /// get the sample rate
    fn sample_rate(&self) -> u16;
    /// get the number of channels
    fn channels(&self) -> Channels;
    /// get the format of the audio samples
    fn sample_format(&self) -> SampleFormat;
    /// try to seek (from current sample) to audio sample offset NOT file byte offset
    fn try_seek(&mut self, sample_offset: i64) -> Result<(), Self::Error>;
    /// get how many samples have been read
    fn played(&self) -> usize;
    /// start back from the first sample
    fn restart(&mut self) -> Result<(), Self::Error> {
        self.try_seek(-(self.played() as i64))
    }
    /// check if EOF
    fn is_eof(&self) -> bool;
}

/// Data type of audio sample encoding
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum SampleFormat {
    /// Signed 8 bit audio
    I8,
    /// Unsigned 8 bit audio
    U8,
    /// Signed 16 bit audio
    I16,
    /// Singed 24 bit audio
    I24,
}

impl SampleFormat {
    /// number of bytes the sample format consumes
    fn size(&self) -> u8 {
        match self {
            SampleFormat::I8 => 1,
            SampleFormat::U8 => 1,
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
        }
    }
}

/// Number and type (interleaved or not) of audio channels
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum Channels {
    Mono,
    Stereo,
}

impl From<Channels> for u16 {
    fn from(val: Channels) -> Self {
        match val {
            Channels::Mono => 1,
            Channels::Stereo => 2,
        }
    }
}

// /// Types of interleaving stereo audio
// pub enum Interleave {

// }

#[derive(Debug)]
pub enum PlatformFileError {
    SeekOutofBounds,
    EOF,
}

/// Platform agnostic file for accessing audio data
pub trait PlatformFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, PlatformFileError>;
    fn seek_from_current(&mut self, offset: i64) -> Result<(), PlatformFileError>;
    fn seek_from_start(&mut self, offset: usize) -> Result<(), PlatformFileError>;
    fn seek_from_end(&mut self, offset: usize) -> Result<(), PlatformFileError>;
    fn length(&mut self) -> usize;
}

#[cfg(feature = "embedded-sdmmc")]
impl<
    D: BlockDevice,
    T: TimeSource,
    const MAX_DIRS: usize,
    const MAX_FILES: usize,
    const MAX_VOLUMES: usize,
> PlatformFile for File<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, PlatformFileError> {
        File::read(self, buf).map_err(|_| PlatformFileError::EOF)
    }

    fn seek_from_current(&mut self, offset: i64) -> Result<(), PlatformFileError> {
        File::seek_from_current(self, offset as i32).map_err(|_| PlatformFileError::SeekOutofBounds)
    }

    fn seek_from_start(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        File::seek_from_start(self, offset as u32).map_err(|_| PlatformFileError::SeekOutofBounds)
    }

    fn seek_from_end(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        File::seek_from_end(self, offset as u32).map_err(|_| PlatformFileError::SeekOutofBounds)
    }

    fn length(&mut self) -> usize {
        File::length(&self) as usize
    }
}

#[cfg(feature = "std")]
impl PlatformFile for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, PlatformFileError> {
        Read::read(self, buf).map_err(|_| PlatformFileError::EOF)
    }

    fn seek_from_current(&mut self, offset: i64) -> Result<(), PlatformFileError> {
        match Seek::seek(self, SeekFrom::Current(offset)) {
            Ok(_) => Ok(()),
            Err(_) => Err(PlatformFileError::SeekOutofBounds),
        }
    }

    fn seek_from_start(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        match Seek::seek(self, SeekFrom::Start(offset as u64)) {
            Ok(_) => Ok(()),
            Err(_) => Err(PlatformFileError::SeekOutofBounds),
        }
    }

    fn seek_from_end(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        match Seek::seek(self, SeekFrom::End(offset as i64)) {
            Ok(_) => Ok(()),
            Err(_) => Err(PlatformFileError::SeekOutofBounds),
        }
    }

    fn length(&mut self) -> usize {
        File::metadata(&self).unwrap().len() as usize
    }
}

#[cfg(test)]
/// Simple wrapper to test file decodes in tests
struct TestFile {
    contents: &'static [u8],
    current_pos: u16,
}

#[cfg(test)]
impl TestFile {
    fn from_bytes(bytes: &'static [u8]) -> Self {
        Self {
            contents: bytes,
            current_pos: 0,
        }
    }
}

#[cfg(test)]
impl PlatformFile for TestFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, PlatformFileError> {
        if self.current_pos as usize == self.contents.len() {
            return Err(PlatformFileError::EOF);
        }
        let read_len = if self.current_pos as usize + buf.len() >= self.contents.len() {
            self.contents.len() - self.current_pos as usize
        } else {
            buf.len()
        };
        let start = self.current_pos as usize;
        for (buf, content) in buf
            .iter_mut()
            .zip(self.contents[start..start + read_len].iter())
        {
            *buf = *content
        }
        self.current_pos += read_len as u16;
        Ok(read_len)
    }

    fn seek_from_current(&mut self, offset: i64) -> Result<(), PlatformFileError> {
        if offset + self.current_pos as i64 > self.contents.len() as i64 {
            return Err(PlatformFileError::SeekOutofBounds);
        }
        if offset.is_positive() {
            self.current_pos += offset as u16;
        } else {
            self.current_pos -= offset as u16;
        }
        Ok(())
    }

    fn seek_from_start(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        if offset > self.contents.len() {
            return Err(PlatformFileError::SeekOutofBounds);
        }
        self.current_pos = offset as u16;
        Ok(())
    }

    fn seek_from_end(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        self.current_pos = (self.contents.len() - offset) as u16;
        Ok(())
    }

    fn length(&mut self) -> usize {
        self.contents.len()
    }
}
