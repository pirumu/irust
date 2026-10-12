//! Lab (c): one table, two physical layouts.
//!
//! * **Row store**: a `Vec<Order>`; the 14 fields of one order sit next to each other in 64 bytes.
//!   Fetching or writing one whole order touches one place; scanning one field still drags all 64
//!   bytes of every row through the cache.
//! * **Column store**: one array per field. A scan that needs two fields streams two arrays and
//!   never reads the other twelve; rebuilding one whole order touches 14 arrays.
//! * **Sorted column + run-length encoding**: if the table is sorted by one column, that column
//!   collapses into (value, start, length) runs and a filter on it becomes a binary search.

use std::hint::black_box;

use crate::rng::Rng;

pub const ROW_BYTES: usize = 64;
pub const EXTRA_COLUMNS: usize = 7;

/// One row of the `orders` table: 7 named columns and 7 more `u32` columns, 64 bytes in total.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    pub id: u64,
    pub ts: u64,
    pub price_cents: u64,
    pub user_id: u32,
    pub quantity: u32,
    pub country: u16,
    pub status: u16,
    pub extra: [u32; EXTRA_COLUMNS],
}

const _: () = assert!(std::mem::size_of::<Order>() == ROW_BYTES);

/// Deterministic synthetic orders; `countries` distinct country codes.
pub fn generate(n: usize, countries: u16, seed: u64) -> Vec<Order> {
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|i| {
            let mut extra = [0u32; EXTRA_COLUMNS];
            for e in &mut extra {
                *e = rng.next_u64() as u32;
            }
            Order {
                id: i as u64,
                ts: 1_700_000_000 + i as u64,
                price_cents: 100 + rng.below(100_000),
                user_id: rng.below(1_000_000) as u32,
                quantity: 1 + rng.below(10) as u32,
                country: rng.below(u64::from(countries)) as u16,
                status: rng.below(5) as u16,
                extra,
            }
        })
        .collect()
}

/// Struct-of-arrays copy of the same table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Columns {
    pub id: Vec<u64>,
    pub ts: Vec<u64>,
    pub price_cents: Vec<u64>,
    pub user_id: Vec<u32>,
    pub quantity: Vec<u32>,
    pub country: Vec<u16>,
    pub status: Vec<u16>,
    pub extra: [Vec<u32>; EXTRA_COLUMNS],
}

impl Columns {
    pub fn from_rows(rows: &[Order]) -> Columns {
        let mut c = Columns::default();
        for r in rows {
            c.id.push(r.id);
            c.ts.push(r.ts);
            c.price_cents.push(r.price_cents);
            c.user_id.push(r.user_id);
            c.quantity.push(r.quantity);
            c.country.push(r.country);
            c.status.push(r.status);
            for (col, v) in c.extra.iter_mut().zip(r.extra) {
                col.push(v);
            }
        }
        c
    }

    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    /// Rebuilds row `i`: 14 reads from 14 different arrays.
    pub fn row(&self, i: usize) -> Order {
        let mut extra = [0u32; EXTRA_COLUMNS];
        for (e, col) in extra.iter_mut().zip(&self.extra) {
            *e = col[i];
        }
        Order {
            id: self.id[i],
            ts: self.ts[i],
            price_cents: self.price_cents[i],
            user_id: self.user_id[i],
            quantity: self.quantity[i],
            country: self.country[i],
            status: self.status[i],
            extra,
        }
    }
}

/// Result of an aggregate query: `SELECT SUM(price_cents), COUNT(*) FROM orders WHERE country = ?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aggregate {
    pub sum_price_cents: u64,
    pub matches: u64,
}

/// Row layout: every row is read in full, although only `country` and `price_cents` are needed.
pub fn aggregate_rows(rows: &[Order], country: u16) -> Aggregate {
    let mut agg = Aggregate { sum_price_cents: 0, matches: 0 };
    for r in rows {
        if r.country == country {
            agg.sum_price_cents += r.price_cents;
            agg.matches += 1;
        }
    }
    agg
}

/// Column layout: only the `country` and `price_cents` arrays are streamed.
pub fn aggregate_columns(cols: &Columns, country: u16) -> Aggregate {
    let mut agg = Aggregate { sum_price_cents: 0, matches: 0 };
    for (&c, &p) in cols.country.iter().zip(&cols.price_cents) {
        if c == country {
            agg.sum_price_cents += p;
            agg.matches += 1;
        }
    }
    agg
}

/// Bytes the loop of [`aggregate_rows`] pulls through the memory system (a model, not a counter).
pub fn bytes_streamed_rows(n: usize) -> u64 {
    (n * ROW_BYTES) as u64
}

/// Bytes streamed by [`aggregate_columns`]: a `u16` and a `u64` per row.
pub fn bytes_streamed_columns(n: usize) -> u64 {
    (n * (2 + 8)) as u64
}

/// Run-length encoding of a column that is sorted: `(value, first position, run length)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rle {
    runs: Vec<(u16, u32, u32)>,
}

impl Rle {
    /// `values` must be sorted ascending.
    pub fn encode(values: &[u16]) -> Rle {
        let mut runs: Vec<(u16, u32, u32)> = Vec::new();
        for (i, &v) in values.iter().enumerate() {
            match runs.last_mut() {
                Some((last, _, len)) if *last == v => *len += 1,
                _ => runs.push((v, i as u32, 1)),
            }
        }
        Rle { runs }
    }

    pub fn runs(&self) -> usize {
        self.runs.len()
    }

    /// `(value: u16, start: u32, len: u32)` per run.
    pub fn encoded_bytes(&self) -> usize {
        self.runs.len() * (2 + 4 + 4)
    }

    /// Positions holding `value`, found by binary search over the runs.
    pub fn range_of(&self, value: u16) -> std::ops::Range<usize> {
        match self.runs.binary_search_by_key(&value, |&(v, _, _)| v) {
            Ok(i) => {
                let (_, start, len) = self.runs[i];
                start as usize..(start + len) as usize
            }
            Err(_) => 0..0,
        }
    }

    pub fn decode(&self) -> Vec<u16> {
        self.runs.iter().flat_map(|&(v, _, len)| std::iter::repeat_n(v, len as usize)).collect()
    }
}

/// Rows sorted by `country` (stable), the layout in which the filter column compresses.
pub fn sorted_by_country(rows: &[Order]) -> Vec<Order> {
    let mut sorted = rows.to_vec();
    sorted.sort_by_key(|r| r.country);
    sorted
}

/// Aggregate over a table sorted by country: binary search in the RLE runs, then sum one slice
/// of the price column.
pub fn aggregate_sorted(cols: &Columns, rle: &Rle, country: u16) -> Aggregate {
    let range = rle.range_of(country);
    Aggregate { sum_price_cents: cols.price_cents[range.clone()].iter().sum(), matches: range.len() as u64 }
}

/// A deterministic sequence of `count` ids in `0..n`, for point lookups.
pub fn lookup_ids(n: usize, count: usize, seed: u64) -> Vec<usize> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| rng.index(n)).collect()
}

/// OLTP-style access: fetch whole rows by id from the row layout.
pub fn fetch_rows(rows: &[Order], ids: &[usize]) -> u64 {
    let mut acc = 0u64;
    for &i in ids {
        let r = black_box(&rows[i]);
        acc = acc.wrapping_add(r.id ^ r.price_cents ^ u64::from(r.extra[6]));
    }
    acc
}

/// The same fetch from the column layout: each row is assembled from 14 arrays.
pub fn fetch_columns(cols: &Columns, ids: &[usize]) -> u64 {
    let mut acc = 0u64;
    for &i in ids {
        let r = black_box(cols.row(i));
        acc = acc.wrapping_add(r.id ^ r.price_cents ^ u64::from(r.extra[6]));
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> (Vec<Order>, Columns) {
        let rows = generate(20_000, 50, 7);
        let cols = Columns::from_rows(&rows);
        (rows, cols)
    }

    #[test]
    fn a_row_is_64_bytes_and_columns_hold_the_same_data() {
        assert_eq!(std::mem::size_of::<Order>(), 64);
        let (rows, cols) = table();
        assert_eq!(cols.len(), rows.len());
        for i in [0, 1, 999, 19_999] {
            assert_eq!(cols.row(i), rows[i]);
        }
        assert!((0..rows.len()).all(|i| cols.row(i) == rows[i]));
    }

    #[test]
    fn both_layouts_give_the_same_answer_to_the_same_query() {
        let (rows, cols) = table();
        for country in [0u16, 7, 49, 50, 1000] {
            let a = aggregate_rows(&rows, country);
            let b = aggregate_columns(&cols, country);
            assert_eq!(a, b, "country {country}");
            let naive: u64 = rows.iter().filter(|r| r.country == country).map(|r| r.price_cents).sum();
            assert_eq!(a.sum_price_cents, naive);
        }
        // Countries 50 and 1000 do not exist.
        assert_eq!(aggregate_rows(&rows, 50), Aggregate { sum_price_cents: 0, matches: 0 });
        let total: u64 = (0..50).map(|c| aggregate_rows(&rows, c).matches).sum();
        assert_eq!(total, rows.len() as u64);
    }

    #[test]
    fn the_column_scan_streams_one_sixth_of_the_bytes() {
        let n = 1_000_000;
        assert_eq!(bytes_streamed_rows(n), 64_000_000);
        assert_eq!(bytes_streamed_columns(n), 10_000_000);
        assert!(bytes_streamed_rows(n) / bytes_streamed_columns(n) >= 6);
    }

    #[test]
    fn rle_round_trips_and_a_sorted_table_answers_by_binary_search() {
        let (rows, _) = table();
        let sorted_rows = sorted_by_country(&rows);
        let sorted_cols = Columns::from_rows(&sorted_rows);
        let rle = Rle::encode(&sorted_cols.country);
        assert_eq!(rle.runs(), 50, "one run per distinct country");
        assert_eq!(rle.decode(), sorted_cols.country);
        assert!(rle.encoded_bytes() * 50 < sorted_cols.country.len() * 2, "RLE is under 2% of the plain u16 column");
        for country in [0u16, 13, 49, 50] {
            assert_eq!(aggregate_sorted(&sorted_cols, &rle, country), aggregate_rows(&rows, country), "country {country}");
        }
        assert_eq!(rle.range_of(200), 0..0);
    }

    #[test]
    fn rle_edge_cases() {
        assert_eq!(Rle::encode(&[]).runs(), 0);
        assert_eq!(Rle::encode(&[5]).decode(), [5]);
        let r = Rle::encode(&[1, 1, 1, 2, 4, 4]);
        assert_eq!(r.runs(), 3);
        assert_eq!(r.range_of(1), 0..3);
        assert_eq!(r.range_of(2), 3..4);
        assert_eq!(r.range_of(3), 0..0);
        assert_eq!(r.range_of(4), 4..6);
    }

    #[test]
    fn point_lookups_agree_between_layouts() {
        let (rows, cols) = table();
        let ids = lookup_ids(rows.len(), 5000, 3);
        assert_eq!(fetch_rows(&rows, &ids), fetch_columns(&cols, &ids));
        assert!(ids.iter().all(|&i| i < rows.len()));
    }
}
