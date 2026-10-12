//! The on-disk record shared by the lab storage engines: the data file of `kvlog`, the write-ahead
//! logs of `lsm` and `pagestore`, and the data blocks of an SSTable.
//!
//! ```text
//! record := crc32c: u32 | key_len: u32 | val_len: u32 | key | value        (little-endian)
//! ```
//!
//! `crc32c` covers every byte after the checksum field. `val_len == TOMBSTONE` marks a deletion
//! and no value bytes follow. Because every record carries its own length and checksum, a reader can
//! tell where a record ends, whether it is intact, and (see [`scan`]) what kind of damage stopped it.

use crate::crc32c::crc32c;

pub const HEADER_LEN: usize = 12;
/// `val_len` of a deletion marker.
pub const TOMBSTONE: u32 = u32::MAX;
pub const MAX_KEY_LEN: usize = 1 << 20;
pub const MAX_VALUE_LEN: usize = 1 << 28;

/// Size on disk of a record.
pub fn encoded_len(key_len: usize, value_len: Option<usize>) -> usize {
    HEADER_LEN + key_len + value_len.unwrap_or(0)
}

/// Appends one record to `out`. `value == None` encodes a tombstone.
///
/// # Panics
/// If the key or value exceeds [`MAX_KEY_LEN`] / [`MAX_VALUE_LEN`]; callers check sizes first.
pub fn encode_into(out: &mut Vec<u8>, key: &[u8], value: Option<&[u8]>) {
    assert!(key.len() <= MAX_KEY_LEN, "key too long");
    assert!(value.is_none_or(|v| v.len() <= MAX_VALUE_LEN), "value too long");
    let start = out.len();
    out.extend_from_slice(&[0; 4]); // checksum, filled in below
    out.extend_from_slice(&(key.len() as u32).to_le_bytes());
    out.extend_from_slice(&value.map_or(TOMBSTONE, |v| v.len() as u32).to_le_bytes());
    out.extend_from_slice(key);
    if let Some(v) = value {
        out.extend_from_slice(v);
    }
    let crc = crc32c(&out[start + 4..]);
    out[start..start + 4].copy_from_slice(&crc.to_le_bytes());
}

/// A record that parsed and verified.
#[derive(Debug, PartialEq, Eq)]
pub struct Record<'a> {
    pub key: &'a [u8],
    /// `None` for a tombstone.
    pub value: Option<&'a [u8]>,
    /// Bytes the record occupies on disk.
    pub len: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parse<'a> {
    Record(Record<'a>),
    /// The buffer ends before the record does (header or body cut short).
    Incomplete,
    /// Header present but its lengths are impossible for a record this program wrote.
    BadHeader,
    /// All bytes of the record are present but the checksum does not match; `len` is the extent
    /// the header claims.
    BadChecksum { len: usize },
}

/// Parses the record at the start of `buf`.
pub fn parse(buf: &[u8]) -> Parse<'_> {
    if buf.len() < HEADER_LEN {
        return Parse::Incomplete;
    }
    let stored = u32::from_le_bytes(buf[0..4].try_into().expect("4 bytes"));
    let key_len = u32::from_le_bytes(buf[4..8].try_into().expect("4 bytes")) as usize;
    let raw_val = u32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
    let val_len = if raw_val == TOMBSTONE { 0 } else { raw_val as usize };
    if key_len > MAX_KEY_LEN || val_len > MAX_VALUE_LEN {
        return Parse::BadHeader;
    }
    let total = HEADER_LEN + key_len + val_len;
    if buf.len() < total {
        return Parse::Incomplete;
    }
    if crc32c(&buf[4..total]) != stored {
        return Parse::BadChecksum { len: total };
    }
    Parse::Record(Record {
        key: &buf[HEADER_LEN..HEADER_LEN + key_len],
        value: (raw_val != TOMBSTONE).then(|| &buf[HEADER_LEN + key_len..total]),
        len: total,
    })
}

/// Why [`scan`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stop {
    /// The buffer ended exactly at a record boundary.
    #[default]
    Clean,
    /// The last record is cut short: the typical shape of a write interrupted by power loss.
    Incomplete,
    /// Everything from the damaged point to the end is zero bytes: the file grew but the data
    /// never reached the disk.
    ZeroTail,
    /// The last record is complete but fails its checksum: a garbled final write.
    BadChecksumAtTail,
    /// A record that has more bytes after it fails its checksum: damage in the middle, not a torn append.
    BadChecksumInMiddle,
    /// A header whose lengths cannot belong to a record we wrote.
    BadHeader,
}

impl Stop {
    /// Shapes of damage that an interrupted append can produce.
    pub fn is_torn_tail(self) -> bool {
        matches!(self, Stop::Incomplete | Stop::ZeroTail | Stop::BadChecksumAtTail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanResult {
    /// Length of the prefix made only of intact records.
    pub valid_len: usize,
    pub stop: Stop,
    pub records: u64,
}

/// Walks `buf` from the start, calling `on_record(offset, record)` for every intact record, and
/// stops at the first damaged one.
pub fn scan(buf: &[u8], mut on_record: impl FnMut(usize, &Record<'_>)) -> ScanResult {
    let mut pos = 0;
    let mut records = 0;
    loop {
        if pos == buf.len() {
            return ScanResult { valid_len: pos, stop: Stop::Clean, records };
        }
        let rest = &buf[pos..];
        let stop = match parse(rest) {
            Parse::Record(r) => {
                on_record(pos, &r);
                pos += r.len;
                records += 1;
                continue;
            }
            _ if rest.iter().all(|&b| b == 0) => Stop::ZeroTail,
            Parse::Incomplete => Stop::Incomplete,
            Parse::BadHeader => Stop::BadHeader,
            Parse::BadChecksum { len } if len == rest.len() => Stop::BadChecksumAtTail,
            Parse::BadChecksum { .. } => Stop::BadChecksumInMiddle,
        };
        return ScanResult { valid_len: pos, stop, records };
    }
}

/// What recovery does with the bytes after the last intact record. The three modes follow the
/// vocabulary of RocksDB's WAL recovery modes (`kAbsoluteConsistency`,
/// `kTolerateCorruptedTailRecords`, `kPointInTimeRecovery`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Recovery {
    /// Any damage, tail or not, is an error.
    Strict,
    /// Cut a damaged *tail* (cannot tell a torn write from rot there); refuse damage that has
    /// bytes after it, because cutting would silently drop records that may be fine.
    #[default]
    TolerateTail,
    /// Stop at the first damaged record and drop everything after it, intact or not.
    PointInTime,
}

/// Recovery refused to continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corrupt {
    /// Offset of the first damaged byte.
    pub offset: u64,
    pub stop: Stop,
}

impl std::fmt::Display for Corrupt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "damaged record at offset {} ({:?})", self.offset, self.stop)
    }
}

impl std::error::Error for Corrupt {}

/// How many bytes to cut from the end of a `total_len`-byte log under `policy`.
pub fn bytes_to_cut(policy: Recovery, scan: &ScanResult, total_len: u64) -> Result<u64, Corrupt> {
    let cut = total_len - scan.valid_len as u64;
    let refuse = Corrupt { offset: scan.valid_len as u64, stop: scan.stop };
    match (scan.stop, policy) {
        (Stop::Clean, _) => Ok(0),
        (_, Recovery::PointInTime) => Ok(cut),
        (stop, Recovery::TolerateTail) if stop.is_torn_tail() => Ok(cut),
        _ => Err(refuse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(key: &[u8], value: Option<&[u8]>) -> Vec<u8> {
        let mut v = Vec::new();
        encode_into(&mut v, key, value);
        v
    }

    #[test]
    fn round_trip_including_empty_key_value_and_tombstone() {
        let cases: [(&[u8], Option<&[u8]>); 5] = [
            (b"k", Some(b"v")),
            (b"", Some(b"")),
            (b"key", Some(b"")),
            (b"", Some(b"value")),
            (b"gone", None),
        ];
        for (k, v) in cases {
            let bytes = enc(k, v);
            assert_eq!(bytes.len(), encoded_len(k.len(), v.map(<[u8]>::len)));
            assert_eq!(parse(&bytes), Parse::Record(Record { key: k, value: v, len: bytes.len() }));
        }
    }

    #[test]
    fn empty_value_and_tombstone_are_different_things() {
        let empty = enc(b"k", Some(b""));
        let tomb = enc(b"k", None);
        assert_ne!(empty, tomb);
        let Parse::Record(a) = parse(&empty) else { panic!() };
        let Parse::Record(b) = parse(&tomb) else { panic!() };
        assert_eq!(a.value, Some(&b""[..]));
        assert_eq!(b.value, None);
    }

    #[test]
    fn every_proper_prefix_is_incomplete_and_every_bit_flip_is_caught() {
        let bytes = enc(b"hello", Some(b"world!"));
        for cut in 0..bytes.len() {
            assert_eq!(parse(&bytes[..cut]), Parse::Incomplete, "prefix {cut}");
        }
        for byte in 0..bytes.len() {
            for bit in 0..8 {
                let mut bad = bytes.clone();
                bad[byte] ^= 1 << bit;
                // A flipped length byte changes the claimed extent, so any non-Record outcome is fine
                // as long as the damage is never accepted.
                assert!(!matches!(parse(&bad), Parse::Record(_)), "byte {byte} bit {bit}");
            }
        }
    }

    #[test]
    fn absurd_lengths_are_rejected_without_allocating() {
        let mut bytes = enc(b"k", Some(b"v"));
        bytes[8..12].copy_from_slice(&(MAX_VALUE_LEN as u32 + 1).to_le_bytes());
        assert_eq!(parse(&bytes), Parse::BadHeader);
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse(&bytes), Parse::BadHeader);
    }

    fn three() -> (Vec<u8>, [usize; 3]) {
        let mut log = Vec::new();
        let mut starts = [0; 3];
        for (i, (k, v)) in [(&b"a"[..], Some(&b"1"[..])), (b"b", Some(b"22")), (b"a", None)]
            .into_iter()
            .enumerate()
        {
            starts[i] = log.len();
            encode_into(&mut log, k, v);
        }
        (log, starts)
    }

    #[test]
    fn scan_visits_records_in_order_and_reports_a_clean_end() {
        let (log, starts) = three();
        let mut seen = Vec::new();
        let r = scan(&log, |off, rec| seen.push((off, rec.key.to_vec(), rec.value.map(<[u8]>::to_vec))));
        assert_eq!(r, ScanResult { valid_len: log.len(), stop: Stop::Clean, records: 3 });
        assert_eq!(seen.iter().map(|s| s.0).collect::<Vec<_>>(), starts);
        assert_eq!(seen[2].2, None);
    }

    #[test]
    fn scan_classifies_the_shape_of_the_damage() {
        let (log, starts) = three();
        let last = starts[2];

        let cut = &log[..log.len() - 3];
        assert_eq!(scan(cut, |_, _| {}).stop, Stop::Incomplete);
        assert_eq!(scan(cut, |_, _| {}).valid_len, last);

        let mut zeros = log.clone();
        zeros.extend_from_slice(&[0; 40]);
        let r = scan(&zeros, |_, _| {});
        assert_eq!((r.stop, r.valid_len, r.records), (Stop::ZeroTail, log.len(), 3));

        let mut garbled_tail = log.clone();
        let n = garbled_tail.len();
        garbled_tail[n - 1] ^= 0xFF;
        let r = scan(&garbled_tail, |_, _| {});
        assert_eq!((r.stop, r.valid_len), (Stop::BadChecksumAtTail, last));

        let mut garbled_middle = log.clone();
        garbled_middle[starts[1] + HEADER_LEN] ^= 0xFF; // key byte of record 2
        let r = scan(&garbled_middle, |_, _| {});
        assert_eq!((r.stop, r.valid_len, r.records), (Stop::BadChecksumInMiddle, starts[1], 1));
    }

    #[test]
    fn policies_decide_what_to_cut() {
        let (log, starts) = three();
        let total = log.len() as u64;

        let torn = scan(&log[..log.len() - 3], |_, _| {});
        let cut = (log.len() - 3 - starts[2]) as u64;
        let torn_total = (log.len() - 3) as u64;
        assert_eq!(bytes_to_cut(Recovery::TolerateTail, &torn, torn_total), Ok(cut));
        assert_eq!(bytes_to_cut(Recovery::PointInTime, &torn, torn_total), Ok(cut));
        assert_eq!(
            bytes_to_cut(Recovery::Strict, &torn, torn_total),
            Err(Corrupt { offset: starts[2] as u64, stop: Stop::Incomplete })
        );

        let mut rot = log.clone();
        rot[starts[1] + HEADER_LEN] ^= 1;
        let mid = scan(&rot, |_, _| {});
        assert_eq!(
            bytes_to_cut(Recovery::TolerateTail, &mid, total),
            Err(Corrupt { offset: starts[1] as u64, stop: Stop::BadChecksumInMiddle })
        );
        assert_eq!(bytes_to_cut(Recovery::PointInTime, &mid, total), Ok(total - starts[1] as u64));

        let clean = scan(&log, |_, _| {});
        for p in [Recovery::Strict, Recovery::TolerateTail, Recovery::PointInTime] {
            assert_eq!(bytes_to_cut(p, &clean, total), Ok(0));
        }
    }

    #[test]
    fn a_valid_record_never_looks_like_zero_fill() {
        // Even the smallest record (empty key, empty value) has a non-zero checksum, so the
        // all-zero test in `scan` cannot mistake real data for a zero tail.
        assert_ne!(enc(b"", Some(b""))[..4], [0, 0, 0, 0]);
        assert_ne!(enc(b"", None)[..4], [0, 0, 0, 0]);
    }
}
