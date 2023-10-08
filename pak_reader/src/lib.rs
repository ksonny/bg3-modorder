use std::io::{Read, Seek, SeekFrom};

use bitflags::bitflags;
use error::ReaderError;
use flate2::read::ZlibDecoder;
use log::error;


mod parser;

#[derive(Debug)]
pub struct LSPKHeader {
    pub version: u32,
    pub offset_dir: u64,
    pub size_dir: u32,
    pub flags: u8,
    pub priority: u8,
    pub hash: [u8; 16],
    pub parts: u16,
}

impl LSPKHeader {
    fn file_entry_size(&self) -> Option<usize> {
        match self.version {
            18 => Some(272usize),
            15 | 16 => Some(2),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct FileListHeader {
    pub count: u32,
    pub size_compressed: u32,
}

const FILE_ENTRY_SIZE: usize = 272usize;

bitflags! {
    #[derive(Debug)]
    pub struct FileEntryFlagsV15: u32 {
        const ZlibCompression = 0x01;
        const LZ4Compression = 0x02;
        const FastCompression = 0x10;
        const DefaultCompression = 0x20;
        const MaxLevelCompression = 0x40;
        const _ = !0;
    }
}

bitflags! {
    #[derive(Debug)]
    pub struct FileEntryFlags: u8 {
        const ZlibCompression = 0x01;
        const LZ4Compression = 0x02;
        const FastCompression = 0x10;
        const DefaultCompression = 0x20;
        const MaxLevelCompression = 0x40;
        const _ = !0;
    }
}

#[derive(Debug)]
pub struct PakFileV15<'a> {
    pub name: &'a [u8],
    pub offset: u64,
    pub size_compressed: u64,
    pub size: u64,
    pub part: u32,
    pub flags: FileEntryFlagsV15,
    pub crc: u32,
    pub unknown2: u32,
}

#[derive(Debug)]
pub struct PakFile<'a> {
    pub name: &'a [u8],
    pub offset: u64,
    pub part: u8,
    pub flags: FileEntryFlags,
    pub size_compressed: u32,
    pub size: u32,
}

mod error {
    #[derive(Debug)]
    pub enum ReaderError {
        Parse(String),
        IO(std::io::Error),
        Decompress(String),
        DecompressMissmatch,
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

pub enum FileEntryIteratorError {
    Parse,
}

pub struct FileEntryIterator<'a> {
    header: &'a LSPKHeader,
    buf: &'a [u8],
}

impl<'a> FileEntryIterator<'a> {
    pub fn new(header: &LSPKHeader, buf: &'a [u8]) -> Self {
        FileEntryIterator { header, buf }
    }
}

impl<'a> Iterator for FileEntryIterator<'a> {
    type Item = Result<PakFile<'a>, FileEntryIteratorError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.len() < FILE_ENTRY_SIZE {
            None
        } else {
            match parse_file_entry(self.buf) {
                Ok((rest, entry)) => {
                    self.buf = rest;
                    Some(Ok(entry))
                }
                Err(e) => {
                    error!("parse error: {}", e);
                    self.buf = &self.buf[FILE_ENTRY_SIZE..];
                    Some(Err(FileEntryIteratorError::Parse))
                }
            }
        }
    }
}

pub struct FileList {
    data: Vec<u8>,
}

impl FileList {
    pub fn iter(&self) -> FileEntryIterator {
        FileEntryIterator::new(&self.data)
    }
}

pub struct PakFileContent {
    pub data: Vec<u8>,
}

impl PakFileContent {
    fn new(data: Vec<u8>) -> Self {
        PakFileContent { data }
    }
}

pub fn read_file_list<F: Read + Seek>(
    stream: &mut F,
    header: &LSPKHeader,
) -> Result<FileList, ReaderError> {
    let mut buf = vec![0u8; header.size_dir as usize];
    stream.seek(SeekFrom::Start(header.offset_dir))?;
    stream.read_exact(&mut buf)?;
    let (rest, list_header) = parse_file_list_header(&buf)?;
    let data = lz4_flex::decompress(rest, FILE_ENTRY_SIZE * list_header.count as usize)?;

    Ok(FileList { data })
}

pub fn read_header<F: Read + Seek>(stream: &mut F) -> Result<LSPKHeader, ReaderError> {
    let mut header_buf = [0u8; LSPKG_HEADER_SIZE];
    stream.seek(SeekFrom::Start(0))?;
    stream.read_exact(&mut header_buf)?;
    let (_, header) = parse_header(&header_buf)?;
    Ok(header)
}

pub fn read_file<F: Read + Seek>(
    stream: &mut F,
    file: &PakFile,
) -> Result<PakFileContent, ReaderError> {
    let mut buf = vec![0u8; file.size_compressed as usize];
    stream.seek(SeekFrom::Start(file.offset))?;
    stream.read_exact(&mut buf)?;

    if file.flags.contains(FileEntryFlags::LZ4Compression) {
        let data = lz4_flex::decompress(&buf, file.size as usize)?;
        Ok(PakFileContent::new(data))
    } else if file.flags.contains(FileEntryFlags::ZlibCompression) {
        let mut decoder = ZlibDecoder::new(buf.as_slice());
        let mut data = Vec::with_capacity(file.size as usize);
        decoder.read_to_end(&mut data)?;
        if data.len() == file.size as usize {
            Ok(PakFileContent::new(data))
        } else {
            Err(ReaderError::DecompressMissmatch)
        }
    } else {
        Ok(PakFileContent::new(buf))
    }
}
