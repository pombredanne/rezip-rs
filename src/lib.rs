#[macro_use]
extern crate error_chain;

extern crate itertools;

#[macro_use]
extern crate lazy_static;

extern crate sha2;

mod bit;
mod circles;
mod code_tree;
mod errors;
pub mod filter;
mod guess;
pub mod gzip;
mod huffman;
pub mod parse;
pub mod serialise;

use bit::BitVec;

pub use errors::*;
pub use parse::parse_deflate;
pub use serialise::compressed_block;
pub use serialise::decompressed_block;


#[derive(Debug, PartialEq, Eq)]
pub enum Code {
    Literal(u8),
    Reference { dist: u16, run_minus_3: u8 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Uncompressed(Vec<u8>),
    FixedHuffman(Vec<Code>),
    DynamicHuffman { trees: BitVec, codes: Vec<Code> },
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::io::Read;
    use std::io::Write;
    use bit::BitWriter;
    use circles::CircularBuffer;
    use ::*;

    #[test]
    fn seq_20_round_trip() {
        // no distance references at all, dynamic huffman
        round_trip(&include_bytes!("../tests/data/seq-20.gz")[..], 51);
    }

    #[test]
    fn lol_round_trip() {
        // fixed huffman, no backreferences
        round_trip(&include_bytes!("../tests/data/lol.gz")[..], 3);
    }

    #[test]
    fn like_love_round_trip() {
        // single true backreference in the middle, fixed huffman
        round_trip(&include_bytes!("../tests/data/like-love.gz")[..], 29);
    }

    #[test]
    fn simple_backreference_round_trip() {
        round_trip(&include_bytes!("../tests/data/abcdef-bcdefg.gz")[..], 13);
    }

    #[test]
    fn libcgi_round_trip() {
        round_trip(
            &include_bytes!("../tests/data/libcgi-untaint-email-perl_0.03.orig.tar.gz")[..],
            20480,
        );
    }

    #[test]
    fn librole_round_trip() {
        round_trip(
            &include_bytes!("../tests/data/librole-basic-perl_0.13-1.debian.tar.gz")[..],
            20480,
        );
    }

    fn round_trip(orig: &[u8], expected_len: usize) {
        let mut raw = Cursor::new(orig);
        let header = gzip::discard_header(&mut raw).unwrap();

        let mut decompressed = Cursor::new(vec![]);
        let mut recompressed = Cursor::new(vec![]);
        recompressed.write_all(&header).unwrap();
        let mut recompressed = BitWriter::new(recompressed);

        {
            let mut dictionary = CircularBuffer::with_capacity(32 * 1024);
            let mut it = parse::parse_deflate(&mut raw).peekable();

            loop {
                let block = match it.next() {
                    Some(block) => block.unwrap(),
                    None => break,
                };

                let last = it.peek().is_none();

                decompressed_block(&mut decompressed, &mut dictionary, &block).unwrap();

                recompressed.write_bit(last).unwrap();
                compressed_block(&mut recompressed, &block).unwrap();

                match block {
                    Block::FixedHuffman(codes) |
                    Block::DynamicHuffman { codes, .. } => guess::guess_huffman(&codes),
                    _ => {}
                }
            }
            recompressed.align().unwrap();
        }

        let mut tail = vec![];
        raw.read_to_end(&mut tail).unwrap();

        let mut recompressed = recompressed.into_inner();
        recompressed.write_all(&tail).unwrap();

        assert_eq!(raw.into_inner().to_vec(), recompressed.into_inner());

        assert_eq!(expected_len, decompressed.into_inner().len());
    }
}
