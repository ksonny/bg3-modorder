use nom::{
    bytes::complete::{tag, take, take_while},
    combinator::{map, map_parser, verify},
    number::complete::{le_u16, le_u32, le_u64, le_u8},
    sequence::tuple,
    IResult,
};

type ParseResult<I, T> = IResult<I, T>;

fn parse_zero_trim_bytes(count: usize) -> impl Fn(&[u8]) -> ParseResult<&[u8], &[u8]> {
    move |input| map_parser(take(count), take_while(|c| c != 0))(input)
}

fn parse_signature(input: &[u8]) -> ParseResult<&[u8], &[u8]> {
    tag([0x4C, 0x53, 0x50, 0x4B])(input)
}

#[derive(Debug)]
pub struct FileListHeader {
    pub count: u32,
    pub size_compressed: u32,
}

mod v15 {
    #[derive(Debug)]
    pub struct PakHeader {
        pub version: u32,
        pub file_list_offset: u64,
        pub file_list_size: u32,
        pub flags: u8,
        pub priority: u8,
        pub hash: [u8; 16],
    }

    #[derive(Debug)]
    pub struct PakFile<'a> {
        pub name: &'a [u8],
        pub offset: u64,
        pub size_compressed: u64,
        pub size: u64,
        pub part: u32,
        pub flags: u32,
        pub crc: u32,
        pub unknown2: u32,
    }
}

mod v16 {
    #[derive(Debug)]
    pub struct PakHeader {
        pub version: u32,
        pub file_list_offset: u64,
        pub file_list_size: u32,
        pub flags: u8,
        pub priority: u8,
        pub hash: [u8; 16],
        pub parts: u16,
    }
}

mod v18 {
    #[derive(Debug)]
    pub struct PakFile<'a> {
        pub name: &'a [u8],
        pub offset_l: u32,
        pub offset_u: u16,
        pub part: u8,
        pub flags: u8,
        pub size_compressed: u32,
        pub size: u32,
    }
}

pub fn parse_header_v15(input: &[u8]) -> ParseResult<&[u8], v15::PakHeader> {
    map(
        tuple((
            parse_signature,
            verify(le_u32, |&v| v == 15),
            le_u64,
            le_u32,
            le_u8,
            le_u8,
            take(16usize),
        )),
        move |(_signature, version, file_list_offset, file_list_size, flags, priority, hash)| {
            v15::PakHeader {
                version,
                file_list_offset,
                file_list_size,
                flags,
                priority,
                hash: hash.try_into().unwrap(),
            }
        },
    )(input)
}

pub fn parse_header_v16_v18(input: &[u8]) -> ParseResult<&[u8], v16::PakHeader> {
    map(
        tuple((
            parse_signature,
            verify(le_u32, |&v| v == 16 || v == 18),
            le_u64,
            le_u32,
            le_u8,
            le_u8,
            take(16usize),
            le_u16,
        )),
        move |(
            _signature,
            version,
            file_list_offset,
            file_list_size,
            flags,
            priority,
            hash,
            parts,
        )| {
            v16::PakHeader {
                version,
                file_list_offset,
                file_list_size,
                flags,
                priority,
                hash: hash.try_into().unwrap(),
                parts,
            }
        },
    )(input)
}

pub fn parse_file_list_header(input: &[u8]) -> ParseResult<&[u8], FileListHeader> {
    map(tuple((le_u32, le_u32)), move |(count, size_compressed)| {
        FileListHeader {
            count,
            size_compressed,
        }
    })(input)
}

pub fn parse_file_entry_v15_v16(input: &[u8]) -> ParseResult<&[u8], v15::PakFile> {
    map(
        tuple((
            parse_zero_trim_bytes(256usize),
            le_u64,
            le_u64,
            le_u64,
            le_u32,
            le_u32,
            le_u32,
            le_u32,
        )),
        move |(name, offset, size, size_compressed, part, flags, crc, unknown2)| v15::PakFile {
            name,
            offset,
            size_compressed,
            size,
            part,
            flags,
            crc,
            unknown2,
        },
    )(input)
}

pub fn parse_file_entry_v18(input: &[u8]) -> ParseResult<&[u8], v18::PakFile> {
    map(
        tuple((
            parse_zero_trim_bytes(256usize),
            le_u32,
            le_u16,
            le_u8,
            le_u8,
            le_u32,
            le_u32,
        )),
        move |(name, offset_l, offset_u, part, flags, size_compressed, size)| v18::PakFile {
            name,
            offset_u,
            offset_l,
            part,
            flags,
            size_compressed,
            size,
        },
    )(input)
}
