use crate::PlatformFile;

/// Wav file parser
pub struct Wav<File: PlatformFile> {
    file: File,
}

impl<File: PlatformFile> Wav<File> {
    pub fn new(file: File) -> Self {
        Self { file }
    }
}
