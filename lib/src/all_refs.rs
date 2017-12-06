use std::collections::HashMap;
use std::iter;

use itertools::Itertools;

use u16_from;
use usize_from;
use Ref;

type Key = (u8, u8, u8);
type BackMap = HashMap<Key, Vec<usize>>;

pub struct AllRefs<'p, 'd> {
    preroll: &'p [u8],
    pub data: &'d [u8],
    map: BackMap,
}

impl<'p, 'd> AllRefs<'p, 'd> {
    pub fn new(preroll: &'p [u8], data: &'d [u8]) -> Self {
        Self {
            preroll,
            data,
            map: whole_map(preroll.iter().chain(data).cloned()),
        }
    }

    pub fn data_len(&self) -> usize {
        self.data.len()
    }

    fn key(&self, data_pos: usize) -> Option<Key> {
        if data_pos + 2 < self.data.len() {
            Some(key_from_bytes(&self.data[data_pos..]))
        } else {
            None
        }
    }

    // None if we are out of possible keys, or Some(possibly empty list)
    pub fn at<'m>(&'m self, data_pos: usize) -> Option<Box<Iterator<Item = Ref> + 'm>> {
        let key = match self.key(data_pos) {
            Some(key) => key,
            None => return None,
        };

        let pos = self.preroll.len() + data_pos;

        // we can only find ourselves, which is invalid, and not handled by (inclusive) range code
        // Maybe I should fix the inclusive range code? Or pretend this is an optimisation.
        if 0 == pos {
            return Some(Box::new(iter::empty()));
        }

        Some(Box::new(
            self.map
                .get(&key)
                .map(|v| {
                    sub_range_inclusive(pos.saturating_sub(32 * 1024), pos.saturating_sub(1), v)
                })
                .unwrap_or(&[])
                .into_iter()
                .rev()
                .map(move |off| {
                    let dist = u16_from(pos - off);
                    let run = self.possible_run_length_at(data_pos, dist);
                    Ref::new(dist, run)
                }),
        ))
    }

    fn get_at_dist(&self, data_pos: usize, dist: u16) -> u8 {
        debug_assert!(dist > 0);
        let pos = data_pos;
        let dist = usize_from(dist);

        if dist <= pos {
            self.data[pos - dist]
        } else {
            self.preroll[self.preroll.len() - (dist - pos)]
        }
    }

    fn possible_run_length_at(&self, data_pos: usize, dist: u16) -> u16 {
        let upcoming_data = &self.data[data_pos..];
        let upcoming_data = &upcoming_data[..258.min(upcoming_data.len())];

        for cur in 3..dist.min(upcoming_data.len() as u16) {
            if upcoming_data[cur as usize] != self.get_at_dist(data_pos, dist - cur) {
                return cur;
            }
        }

        for cur in dist..(upcoming_data.len() as u16) {
            if upcoming_data[(cur % dist) as usize] != upcoming_data[cur as usize] {
                return cur;
            }
        }

        upcoming_data.len() as u16
    }
}

fn key_from_bytes(from: &[u8]) -> Key {
    (from[0], from[1], from[2])
}

fn sub_range_inclusive(start: usize, end: usize, range: &[usize]) -> &[usize] {
    let end_idx = match range.binary_search(&end) {
        Ok(e) => e + 1,
        Err(e) => e,
    };

    let range = &range[..end_idx];

    let start_idx = match range.binary_search(&start) {
        Ok(e) => e,
        Err(e) => e,
    };

    &range[start_idx..]
}

fn whole_map<I: Iterator<Item = u8>>(data: I) -> BackMap {
    let mut map = BackMap::with_capacity(32 * 1024);

    for (pos, keys) in data.tuple_windows::<Key>().enumerate() {
        map.entry(keys).or_insert_with(|| Vec::new()).push(pos);
    }

    map
}

#[cfg(test)]
mod tests {
    #[test]
    fn sub_range() {
        use super::sub_range_inclusive as s;
        assert_eq!(&[5, 6], s(5, 6, &[4, 5, 6, 7]));
        assert_eq!(&[5, 6], s(5, 6, &[5, 6, 7]));
        assert_eq!(&[5, 6], s(5, 6, &[4, 5, 6]));

        assert_eq!(&[5, 6], s(4, 7, &[2, 3, 5, 6, 8, 9]));
        assert_eq!(&[5, 6], s(4, 7, &[5, 6, 8, 9]));
        assert_eq!(&[5, 6], s(4, 7, &[2, 3, 5, 6]));

        assert_eq!(&[0usize; 0], s(7, 8, &[4, 5, 6]));
        assert_eq!(&[0usize; 0], s(7, 8, &[9, 10]));
        assert_eq!(&[0usize; 0], s(7, 8, &[]));
    }
}
