use std::collections::HashMap;
use std::fmt;
use std::mem;

use circles::CircularBuffer;
use errors::*;
use serialise;

use Code;

pub fn guess_huffman(codes: &[Code]) {
    println!("{:?}", max_distance(codes))
}

fn max_distance(codes: &[Code]) -> Option<u16> {
    codes
        .iter()
        .flat_map(|code| if let Code::Reference { dist, .. } = *code {
            Some(dist)
        } else {
            None
        })
        .max()
}

/// checks if any code references before the start of this block
fn outside_range(codes: &[Code]) -> bool {
    codes.iter().enumerate().any(|(pos, code)| {
        if let Code::Reference { dist, .. } = *code {
            dist as usize >= pos // off-by-one?
        } else {
            false
        }
    })
}

fn single_block_mem(window_size: u16, codes: &[Code]) -> Vec<Code> {
    let mut ret = Vec::with_capacity(codes.len());
    single_block_encode_helper(
        window_size,
        serialise::DecompressedBytes::new(codes.iter()),
        |code| {
            ret.push(code);
            Ok(())
        },
    ).expect("fails only if closure fails");

    ret
}

fn single_block_encode(window_size: u16, codes: &[Code]) -> Result<()> {
    let mut expected = codes.iter();

    use Code::*;
    let mut seen = 0usize;

    single_block_encode_helper(
        window_size,
        serialise::DecompressedBytes::new(codes.iter()),
        |code| {
            seen += 1;

            match expected.next() {
                Some(&Literal(expected_byte)) => {
                    match code {
                        Literal(byte) => {
                            ensure!(
                                expected_byte == byte,
                                "emitted the wrong literal, 0x{:02x} != 0x{:02x} ({:?} != {:?})",
                                expected_byte,
                                byte,
                                expected_byte as char,
                                byte as char,
                            );
                            Ok(())
                        }
                        Reference { dist, run_minus_3 } => {
                            let run = u16::from(run_minus_3) + 3;
                            bail!(
                                "we found a run ({}, {}) that the original encoder missed",
                                dist,
                                run
                            )
                        }
                    }
                }
                Some(&Reference {
                         dist: expected_dist,
                         run_minus_3,
                     }) => {
                    let expected_run = u16::from(run_minus_3) + 3;

                    match code {
                        Literal(byte) => {
                            bail!(
                                "we failed to spot the ({}, {}) backreference, wrote a 0x{:02x} literal instead",
                                expected_dist,
                                expected_run,
                                byte
                            )
                        }
                        Reference { dist, run_minus_3 } => {
                            let run = u16::from(run_minus_3) + 3;
                            if expected_dist != dist || expected_run != run {
                                bail!(
                                    "we found a different run: ({}, {}) != ({}, {})",
                                    expected_dist,
                                    expected_run,
                                    dist,
                                    run,
                                );
                            }
                            Ok(())
                        }
                    }
                }
                None => bail!("we emitted a code that isn't supposed to be there"),
            }
        },
    )?;

    ensure!(
        seen == codes.len(),
        "wrong number of codes were emitted, expected: {} != {}",
        codes.len(),
        seen
    );

    Ok(())
}

/// Unlike Peekable, this is not lazy.
struct ThreePeek<I: Iterator> {
    inner: I,
    second: Option<I::Item>,
    third: Option<I::Item>,
}

impl<I: Iterator> Iterator for ThreePeek<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        mem::replace(
            &mut self.second,
            mem::replace(&mut self.third, self.inner.next()),
        )
    }
}

impl<I: Iterator> ThreePeek<I>
where
    I::Item: Copy,
{
    fn new(mut inner: I) -> Self {
        let second = inner.next();
        let third = inner.next();
        ThreePeek {
            inner,
            second,
            third,
        }
    }

    fn next_three(&mut self) -> Option<(I::Item, I::Item, I::Item)> {
        if let Some(first) = self.second {
            if let Some(second) = self.third {
                self.next().unwrap();
                if let Some(third) = self.third {
                    return Some((first, second, third));
                }
            }
        }

        return None;
    }
}

fn single_block_encode_helper<B: Iterator<Item = u8>, F>(
    window_size: u16,
    coderator: B,
    mut emit: F,
) -> Result<()>
where
    F: FnMut(Code) -> Result<()>,
{
    let mut coderator = coderator.enumerate().peekable();
    let mut buf = CircularBuffer::with_capacity(32 * 1024 + 258 + 3);
    let mut map = HashMap::new();
    let mut key = Key::default();

    loop {
        println!(".");

        let (pos, byte) = match coderator.next() {
            Some(x) => x,
            None => return Ok(()),
        };

        let evicted = key.push(byte);
        buf.append(byte);

        println!("evicted: {}", evicted as char);

        if pos < 2 {
            // don't confuse the map code with a half-initialised key
            continue;
        } else if pos > 2 {
            emit(Code::Literal(evicted))?;
        }

        //        #[cfg(never)]
        println!("pos: {}, key: {:?}, map: {:?}", pos, key, map);

        // the map tracks pointers to the *end* of where the block is,
        // as this removes a load of +1s and -2s from the code, not because
        // it's essentially very clear. I think.

        let old = match map.insert(key, pos) {
            Some(old) => old,
            None => {
                continue;
            }
        };

        //        #[cfg(never)]
        println!(
            "think we've found a run, we're at {} and the old was at {}",
            pos,
            old
        );

        let dist = pos - old;

        if dist > (window_size as usize) {
            // TODO: off-by-one
            continue;
        }

        let dist = dist as u16;

        let mut run = 3u16;

        loop {
            if run >= 258 {
                assert_eq!(258, run);
                break;
            }

            let &(pos, byte) = coderator.peek().expect("TODO");

            //            #[cfg(never)]
            println!("{:?} != {:?}", buf.get_at_dist(dist) as char, byte as char);

            if buf.get_at_dist(dist) != byte {
                break;
            }

            let (pos, byte) = coderator.next().expect("consuming peek'd value");

            key.push(byte);
            buf.append(byte);
            map.insert(key, pos);

            run += 1;
        }

        emit(Code::Reference {
            dist,
            run_minus_3: (run - 3) as u8,
        })?;

        // and reset the state inside 'key' to what the rest of the code expects
        for waste in 0..3 {
            let (pos, byte) = match coderator.next() {
                Some(val) => val,
                None => {
                    // hit the end of the stream, flush the key as literals

                    println!("end!");

                    for i in 0..waste {
                        emit(Code::Literal(key.push(0xff)))?;
                    }

                    return Ok(());
                }
            };

            println!("waste: {:?}", key.push(byte) as char);
            buf.append(byte);
            map.insert(key, pos);
        }
    }
}

#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
struct Key {
    vals: [u8; 3],
}

impl Key {
    fn push(&mut self, val: u8) -> u8 {
        let evicted = self.vals[0];
        self.vals[0] = self.vals[1];
        self.vals[1] = self.vals[2];
        self.vals[2] = val;
        evicted
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Key{{ {:?} {:?} {:?} 0x{:02x}{:02x}{:02x}}}",
            self.vals[0] as char,
            self.vals[1] as char,
            self.vals[2] as char,
            self.vals[0],
            self.vals[1],
            self.vals[2],
        )
    }
}


#[cfg(never)]
fn search(window_size: u16, old_data: &[u8], codes: &[Code]) -> Result<Option<(u16, u16)>> {

    let data = {
        use std::io::Cursor;
        use std::io::Write;

        let mut data = Cursor::new(Vec::with_capacity(old_data.len() + codes.len()));
        data.write_all(old_data)?;

        let mut dictionary = CircularBuffer::with_capacity(32 * 1024);
        serialise::decompressed_codes(&mut data, &mut dictionary, codes)?;
        data.into_inner()
    };

    let run_max = 256 + 3;

    let start = old_data.len();

    let mut pos = 0;
    while start + pos < data.len() {
        let window = &data[start.saturating_sub(usize_from(window_size))..start + pos];
        let next_three = window[pos..pos + 3];
        window.windows(3).filter(|window| next_three == window);

        // this is dumb

        pos += 1;
    }

    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use parse;
    use Block;

    #[test]
    fn find_single_ref_from_file() {
        match parse::parse_deflate(Cursor::new(
            &include_bytes!("../tests/data/abcdef-bcdefg.gz")[10..],
        )).next() {
            Some(Ok(Block::FixedHuffman(codes))) => single_block_encode(32, &codes).unwrap(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn find_single_lits() {
        use Code::Literal as L;
        use Code::Reference as R;
        let exp = &[
            L(b'a'),
            L(b'b'),
            L(b'c'),
            L(b'd'),
            L(b'e'),
            L(b'f'),
            L(b' '),
            R {
                dist: 6,
                run_minus_3: 2,
            },
            L(b'g'),
            L(b'h'),
            L(b'i'),
        ];
        assert_eq!(exp, single_block_mem(32, exp).as_slice());
    }

    #[test]
    fn three() {
        let a: Vec<u8> = (0..7).collect();
        let mut it = ThreePeek::new(a.into_iter());
        assert_eq!(Some(0), it.next());
        assert_eq!(Some((1, 2, 3)), it.next_three());
        assert_eq!(Some((2)), it.next());
        assert_eq!(Some((3)), it.next());
        assert_eq!(Some((4, 5, 6)), it.next_three());
        assert_eq!(None, it.next_three());
        assert_eq!(Some((5)), it.next());
    }
}

fn usize_from(val: u16) -> usize {
    val as usize
}
