use nom::{
    bytes::complete::{tag, take, take_while},
    combinator::{map, map_parser, verify},
    number::complete::{le_u16, le_u32, le_u64, le_u8},
    sequence::tuple, IResult,
};

use crate::{LSPKHeader, FileListHeader, PakFile, FileEntryFlags, FileEntryFlagsV15, PakFileV15};

type ParseResult<I, T> = IResult<I, T>;

fn parse_zero_trim_bytes(count: usize) -> impl Fn(&[u8]) -> ParseResult<&[u8], &[u8]> {
    move |input| map_parser(take(count), take_while(|c| c != 0))(input)
}

fn parse_signature(input: &[u8]) -> ParseResult<&[u8], &[u8]> {
    tag([0x4C, 0x53, 0x50, 0x4B])(input)
}

pub fn parse_header_v15(input: &[u8]) -> ParseResult<&[u8], LSPKHeader> {
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
        move |(_signature, version, offset_dir, size_dir, flags, priority, hash)| {
            LSPKHeader {
                version,
                offset_dir,
                size_dir,
                flags,
                priority,
                hash: hash.try_into().unwrap(),
                parts: 1,
            }
        },
    )(input)
}

pub fn parse_header_v16_v18(input: &[u8]) -> ParseResult<&[u8], LSPKHeader> {
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

fn parse_file_list_header(input: &[u8]) -> ParseResult<&[u8], FileListHeader> {
    map(tuple((le_u32, le_u32)), move |(count, size_compressed)| {
        FileListHeader {
            count,
            size_compressed,
        }
    })(input)
}

fn parse_file_entry_v15_v16(input: &[u8]) -> ParseResult<&[u8], PakFileV15> {
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
        move |(name, offset, size, size_compressed, part, flags, crc, unknown2)| PakFileV15 {
            name,
            offset,
            size_compressed,
            size,
            part,
            flags: FileEntryFlagsV15::from_bits(flags).unwrap(),
            crc,
            unknown2
        },
    )(input)
}

fn parse_file_entry_v18(input: &[u8]) -> ParseResult<&[u8], PakFile> {
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
        move |(name, offset_l, offset_u, part, flags, size_compressed, size)| PakFile {
            name,
            offset: (offset_l as u64) | (offset_u as u64) << 32,
            part,
            flags: FileEntryFlags::from_bits(flags).unwrap(),
            size_compressed,
            size,
        },
    )(input)
}
