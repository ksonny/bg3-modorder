use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

use clap::Parser;
use env_logger::Env;
use flate2::read::ZlibDecoder;
use log::{error, info};
use nom::{
    bytes::complete::{tag, take},
    combinator::map,
    number::complete::{le_u16, le_u32, le_u64, le_u8},
    sequence::tuple,
    IResult,
};
use bitflags::bitflags;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    path: PathBuf,
}

const LSPKG_HEADER_SIZE: usize = 44usize;

#[derive(Debug)]
struct LSPKHeader {
    version: u32,
    offset_dir: u64,
    size_dir: u32,
    flags: u8,
    priority: u8,
    hash: [u8; 16],
    parts: u16,
}

const FILE_LIST_HEADER_SIZE: usize = 8usize;

#[derive(Debug)]
struct FileListHeader {
    count: u32,
    size_compressed: u32,
}

const FILE_ENTRY_SIZE: usize = 272usize;

bitflags! {
    #[derive(Debug)]
    struct FileEntryFlags: u8 {
        const ZlibCompression = 0x01;
        const LZ4Compression = 0x02;
        const FastCompression = 0x10;
        const DefaultCompression = 0x20;
        const MaxLevelCompression = 0x40;
        const _ = !0;
    }
}

#[derive(Debug)]
struct PakFile<'a> {
    name: &'a [u8],
    offset: u64,
    part: u8,
    flags: FileEntryFlags,
    size_compressed: u32,
    size: u32,
}

#[derive(Debug)]
enum ReaderError {
    ParseFileListHeader,
    ParseHeader,
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

fn parse_signature(input: &[u8]) -> IResult<&[u8], &[u8]> {
    tag([0x4C, 0x53, 0x50, 0x4B])(input)
}

fn parse_header(input: &[u8]) -> IResult<&[u8], LSPKHeader> {
    map(
        tuple((
            parse_signature,
            le_u32,
            le_u64,
            le_u32,
            le_u8,
            le_u8,
            take(16usize),
            le_u16,
        )),
        move |(_signature, version, offset_dir, size_dir, flags, priority, hash, parts)| {
            LSPKHeader {
                version,
                offset_dir,
                size_dir,
                flags,
                priority,
                hash: hash.try_into().unwrap(),
                parts,
            }
        },
    )(input)
}

fn parse_file_list_header(input: &[u8]) -> IResult<&[u8], FileListHeader> {
    map(tuple((le_u32, le_u32)), move |(count, size_compressed)| {
        FileListHeader {
            count,
            size_compressed,
        }
    })(input)
}

fn parse_file_entry(input: &[u8]) -> IResult<&[u8], PakFile> {
    map(
        tuple((
            take(256usize),
            le_u32,
            le_u16,
            le_u8,
            le_u8,
            le_u32,
            le_u32,
        )),
        move |(name, offset_l, offset_u, part, flags, size_compressed, size)| PakFile {
            name,
            offset: (offset_l as u64) | (offset_u as u64) << 32,
            part,
            flags: FileEntryFlags::from_bits(flags).unwrap(),
            size_compressed,
            size
        },
    )(input)
}

enum FileEntryIteratorError {
    Parse,
}

struct FileEntryIterator<'a> {
    buf: &'a [u8],
}

impl<'a> FileEntryIterator<'a> {
    fn new(buf: &'a [u8]) -> Self {
        FileEntryIterator { buf }
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

struct FileList {
    data: Vec<u8>
}

impl FileList {
    fn iter<'a>(&'a self) -> FileEntryIterator<'a> {
        FileEntryIterator::new(&self.data)
    }
}

struct PakFileContent {
    data: Vec<u8>
}
impl PakFileContent {
    fn new(data: Vec<u8>) -> Self {
        PakFileContent { data }
    }
}

fn read_file_list<F: Read + Seek>(stream: &mut F, header: &LSPKHeader) -> Result<FileList, ReaderError> {
    let mut buf = vec![0u8; header.size_dir as usize];
    stream.seek(SeekFrom::Start(header.offset_dir))?;
    stream.read(&mut buf)?;
    let (rest, list_header) = parse_file_list_header(&buf)?;
    let data = lz4_flex::decompress(rest, FILE_ENTRY_SIZE * list_header.count as usize)?;

    Ok(FileList { data })
}

fn read_header<F: Read + Seek>(stream: &mut F) -> Result<LSPKHeader, ReaderError> {
    let mut header_buf = [0u8; LSPKG_HEADER_SIZE];
    stream.seek(SeekFrom::Start(0))?;
    stream.read(&mut header_buf)?;
    let (_, header) = parse_header(&header_buf)?;
    Ok(header)
}

fn read_file<F: Read + Seek>(stream: &mut F, file: &PakFile) -> Result<PakFileContent, ReaderError> {
    let mut buf = vec![0u8; file.size_compressed as usize];
    stream.seek(SeekFrom::Start(file.offset))?;
    stream.read(&mut buf)?;

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


mod ModMeta {
    use serde::Serialize;

    #[derive(Debug, Serialize, PartialEq)]
    struct Save {
        version: Version,
        region: Region
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct Version {
        major: u32,
        minor: u32,
        revision: u32,
        build: u32,
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct Region {
        id: String,

        #[serde(rename = "$value")]
        node: Node,
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct Node {
        #[serde(rename = "$value")]
        entries: Vec<NodeEntry>
    }

    #[derive(Debug, Serialize, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    enum NodeEntry {
        Attribute(NodeAttribute),
        Children(NodeChildren)
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct NodeAttribute {
        id: String,
        #[serde(rename = "type")]
        node_type: NodeType,
    }

    #[derive(Debug, Serialize, PartialEq)]
    enum NodeType {
        LSWString,
        LSString,
        FixedString,
        #[serde(rename = "int32")]
        Int32,
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct NodeChildren {
        #[serde(rename = "$value")]
        nodes: Vec<Node>
    }

    #[cfg(test)]
    mod tests {
    }
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();

    let mut file = File::open(args.path)?;

    let header = read_header(&mut file)?;
    info!("header hash: {:?}", header.hash);

    let file_list = read_file_list(&mut file, &header)?;

    let mut count = 0;
    for entry in file_list.iter() {
        if let Ok(entry) = entry {
            count += 1;
            let name = std::str::from_utf8(entry.name)?;
            if name.contains("/meta.lsx") {
                info!("f: {} {:?}", name, entry.flags);
                let content = read_file(&mut file, &entry)?;
                info!("c: {}", std::str::from_utf8(&content.data)?);
            }

        }
    }

    info!("files: {}", count);

    Ok(())
}
