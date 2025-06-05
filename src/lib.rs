#![cfg_attr(not(feature = "std"), no_std)]

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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()>;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u32;
    fn encoding(&self) -> Encoding;
}

/// Audio Encoding in terms of bit depth
#[derive(Debug)]
pub enum Encoding {
    /// Unsigned 8 bit audio
    U8Bit(u8),
    /// Signed 16 bit audio
    S16Bit(i16),
    /// Singed 24 bit audio
    S24Bit(i32),
}

pub enum PlatformFileErrors {
    FailedRead,
    FailedSeek,
    FailedToGetLen,
}

/// Platform agnostic file for accessing audio data
pub trait PlatformFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()>;
    fn seek_from_current(&mut self, offset: usize) -> Result<(), ()>;
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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        File::read(self, buf).map_err(|_| ())
    }
    fn seek_from_current(&mut self, offset: usize) -> Result<(), ()> {
        File::seek_from_current(self, offset as i32).map_err(|_| ())
    }
    fn length(&mut self) -> usize {
        File::length(&self) as usize
    }
}

#[cfg(feature = "std")]
impl PlatformFile for File<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        Read::read(self, buf).map_err(|_| ())
    }
    fn seek_from_current(&mut self, offset: usize) -> Result<(), ()> {
        Seek::seek(self, SeekFrom::Current(offset as i64)).map_err(|_| ());
        Ok(())
    }
    fn length(&mut self) -> usize {
        File::metadata(&self).unwrap().len() as usize
    }
}
