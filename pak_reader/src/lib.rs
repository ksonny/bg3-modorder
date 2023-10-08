use std::{
    io::{Read, Seek, SeekFrom}
};

use bitflags::bitflags;
use error::ReaderError;
use flate2::read::ZlibDecoder;
use parser::{
    parse_file_entry_v15_v16, parse_file_entry_v18, parse_file_list_header, parse_header_v15,
    parse_header_v16_v18,
};

mod parser;

mod error {
    #[derive(Debug)]
    pub enum ReaderError {
        Parse(String),
        IO(std::io::Error),
        Decompress(String),
        DecompressMissmatch,
        HeaderParseError,
        UnsupportedVersion,
    }

    impl std::fmt::Display for ReaderError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    impl std::error::Error for ReaderError {}

    impl From<std::io::Error> for ReaderError {
        fn from(e: std::io::Error) -> Self {
            ReaderError::IO(e)
        }
    }

    impl From<lz4_flex::block::DecompressError> for ReaderError {
        fn from(e: lz4_flex::block::DecompressError) -> Self {
            ReaderError::Decompress(format!("{}", e))
        }
    }

    impl From<nom::Err<nom::error::Error<&[u8]>>> for ReaderError {
        fn from(e: nom::Err<nom::error::Error<&[u8]>>) -> Self {
            ReaderError::Parse(format!("{:?}", e))
        }
    }
}

bitflags! {
    #[derive(Debug)]
    pub struct FileEntryFlags: u32 {
        const ZlibCompression = 0x01;
        const LZ4Compression = 0x02;
        const FastCompression = 0x10;
        const DefaultCompression = 0x20;
        const MaxLevelCompression = 0x40;
        const _ = !0;
    }
}

pub struct Package<F: Read + Seek> {
    file: F,
}

pub struct PackageFile<'a> {
    pub name: &'a [u8],
    pub offset: u64,
    pub size_compressed: usize,
    pub size: usize,
    pub flags: FileEntryFlags,
}

enum PackageFileVersion {
    V15,
    V18,
}

pub struct PackageFiles {
    v: PackageFileVersion,
    data: Vec<u8>,
}

impl PackageFiles {
    pub fn iter<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Result<PackageFile<'a>, ReaderError>> + 'a> {
        if let PackageFileVersion::V15 = self.v {
            Box::new(PackageFilesV15Iterator { data: &self.data })
        } else {
            Box::new(PackageFilesV18Iterator { data: &self.data })
        }
    }
}

pub struct PackageFilesV15Iterator<'a> {
    data: &'a [u8],
}

pub struct PackageFilesV18Iterator<'a> {
    data: &'a [u8],
}

impl<F: Read + Seek> Package<F> {
    pub fn new(file: F) -> Self {
        Package { file }
    }

    pub fn files(&mut self) -> Result<PackageFiles, ReaderError> {
        let mut header_buf = [0; 44usize];
        self.file.read_exact(&mut header_buf)?;

        let (version, file_list_offset, file_list_size) =
            if let Ok((_, header)) = parse_header_v16_v18(&header_buf) {
                (
                    header.version,
                    header.file_list_offset,
                    header.file_list_size as usize,
                )
            } else {
                let (_, header) = parse_header_v15(&header_buf)?;
                (
                    header.version,
                    header.file_list_offset,
                    header.file_list_size as usize,
                )
            };

        let (v, entry_size) = match version {
            15 | 16 => Ok((PackageFileVersion::V15, 296usize)),
            18 => Ok((PackageFileVersion::V18, 272usize)),
            _ => Err(ReaderError::UnsupportedVersion),
        }?;

        let mut buf = vec![0u8; file_list_size];
        self.file.seek(SeekFrom::Start(file_list_offset))?;
        self.file.read_exact(&mut buf)?;
        let (rest, list_header) = parse_file_list_header(&buf)?;
        let data = lz4_flex::decompress(rest, entry_size * list_header.count as usize)?;

        Ok(PackageFiles { v, data })
    }

    pub fn content(&mut self, file: &PackageFile) -> Result<Vec<u8>, ReaderError> {
        let mut buf = vec![0u8; file.size_compressed];
        self.file.seek(SeekFrom::Start(file.offset))?;
        self.file.read_exact(&mut buf)?;

        if file.flags.contains(FileEntryFlags::LZ4Compression) {
            let data = lz4_flex::decompress(&buf, file.size)?;
            Ok(data)
        } else if file.flags.contains(FileEntryFlags::ZlibCompression) {
            let mut decoder = ZlibDecoder::new(buf.as_slice());
            let mut data = Vec::with_capacity(file.size);
            decoder.read_to_end(&mut data)?;
            if data.len() == file.size {
                Ok(data)
            } else {
                Err(ReaderError::DecompressMissmatch)
            }
        } else {
            Ok(buf)
        }
    }
}

impl<'a> Iterator for PackageFilesV15Iterator<'a> {
    type Item = Result<PackageFile<'a>, ReaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry_size = 296usize;

        if self.data.len() >= entry_size {
            let f = match parse_file_entry_v15_v16(self.data) {
                Ok((_, entry)) => {
                    self.data = &self.data[entry_size..];
                    Ok(PackageFile {
                        name: entry.name,
                        offset: entry.offset,
                        size_compressed: entry.size_compressed as usize,
                        size: entry.size as usize,
                        flags: FileEntryFlags::from_bits(entry.flags).unwrap(),
                    })
                }
                Err(e) => Err(e.into()),
            };
            Some(f)
        } else {
            None
        }
    }
}

impl<'a> Iterator for PackageFilesV18Iterator<'a> {
    type Item = Result<PackageFile<'a>, ReaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry_size = 272usize;

        if self.data.len() >= entry_size {
            let f = match parse_file_entry_v18(self.data) {
                Ok((_, entry)) => {
                    self.data = &self.data[entry_size..];
                    Ok(PackageFile {
                        name: entry.name,
                        offset: entry.offset_l as u64 | (entry.offset_u as u64) << 32,
                        size_compressed: entry.size_compressed as usize,
                        size: entry.size as usize,
                        flags: FileEntryFlags::from_bits(entry.flags as u32).unwrap(),
                    })
                }
                Err(e) => Err(e.into()),
            };
            Some(f)
        } else {
            None
        }
    }
}
