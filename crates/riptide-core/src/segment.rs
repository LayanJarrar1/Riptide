//! Segments and the segment planner.
//!
//! A download of known size is split into byte ranges that are fetched concurrently. The planner is
//! a pure function so it can be property-tested:
//!
//! * [`plan_segments`] — initial plan: `min(connections, size / 1 MiB)` equal ranges covering
//!   `[0, size)` exactly;
//! * [`split_segment`] — work stealing: when a connection finishes, the largest remaining range is
//!   split in two if its remainder is at least 2 MiB.

use serde::{Deserialize, Serialize};

use crate::download::DownloadId;

/// Smallest segment the initial plan creates (1 MiB).
pub const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;
/// A running segment is only split if at least this much is still unwritten (2 MiB).
pub const MIN_SPLIT_REMAINDER_BYTES: u64 = 2 * 1024 * 1024;

/// Half-open byte range `[start, end)`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ByteRange {
    /// First byte, inclusive.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start: u64,
    /// One past the last byte.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end: u64,
}

impl ByteRange {
    /// `[start, end)`.
    pub const fn new(start: u64, end: u64) -> Self {
        ByteRange { start, end }
    }

    /// Number of bytes in the range.
    pub const fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// `true` when `start == end`.
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Value for an HTTP `Range` header (`bytes=start-last`), inclusive on both ends as HTTP wants.
    pub fn http_range_value(&self) -> String {
        format!("bytes={}-{}", self.start, self.end.saturating_sub(1))
    }
}

/// Lifecycle of one segment.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    /// Planned, no connection yet.
    Pending,
    /// A connection is writing it.
    Active,
    /// `written == len`.
    Done,
    /// The connection failed; the scheduler will retry from `start + written`.
    Failed,
}

impl SegmentState {
    /// Stable lower-case name used in the database.
    pub const fn name(self) -> &'static str {
        match self {
            SegmentState::Pending => "pending",
            SegmentState::Active => "active",
            SegmentState::Done => "done",
            SegmentState::Failed => "failed",
        }
    }

    /// Inverse of [`Self::name`].
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "pending" => Some(SegmentState::Pending),
            "active" => Some(SegmentState::Active),
            "done" => Some(SegmentState::Done),
            "failed" => Some(SegmentState::Failed),
            _ => None,
        }
    }
}

/// One segment of a download as persisted in the `segments` table.
///
/// `written` counts bytes already on disk from `start`; the engine resumes at
/// `start + written`. `end` is exclusive.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    /// Owning download.
    pub download_id: DownloadId,
    /// Position in the download's segment list (0-based, unique per download).
    pub index: u32,
    /// First byte, inclusive.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start: u64,
    /// One past the last byte.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end: u64,
    /// Bytes already on disk from `start`.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub written: u64,
    /// State.
    pub state: SegmentState,
}

impl Segment {
    /// A pending segment covering `range`.
    pub const fn pending(download_id: DownloadId, index: u32, range: ByteRange) -> Self {
        Segment {
            download_id,
            index,
            start: range.start,
            end: range.end,
            written: 0,
            state: SegmentState::Pending,
        }
    }

    /// The full `[start, end)` range.
    pub const fn range(&self) -> ByteRange {
        ByteRange::new(self.start, self.end)
    }

    /// Total bytes in the segment.
    pub const fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// `true` for a zero-length segment (only a 0-byte file has one).
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes still to fetch.
    pub const fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.written)
    }

    /// Where the next byte goes: `start + written`.
    pub const fn next_offset(&self) -> u64 {
        self.start + self.written
    }

    /// The range still to fetch, `[start + written, end)`.
    pub const fn remaining_range(&self) -> ByteRange {
        ByteRange::new(self.next_offset(), self.end)
    }

    /// `true` once every byte is on disk.
    pub const fn is_complete(&self) -> bool {
        self.written >= self.len()
    }
}

/// Initial plan for a download of `size` bytes with at most `connections` connections.
///
/// Rules:
/// * number of segments `n = clamp(size / min_segment, 1, connections)`;
/// * the ranges are contiguous, non-overlapping and cover `[0, size)` exactly;
/// * sizes differ by at most one byte (the first `size % n` ranges are one byte longer);
/// * every range is at least `min_segment` long unless `n == 1`;
/// * `size == 0` yields one empty range `[0, 0)`; `connections == 0` or `min_segment == 0` are
///   treated as `1`.
pub fn plan_segments(size: u64, connections: u32, min_segment: u64) -> Vec<ByteRange> {
    let connections = u64::from(connections.max(1));
    let min_segment = min_segment.max(1);
    if size == 0 {
        return vec![ByteRange::new(0, 0)];
    }
    let n = (size / min_segment).clamp(1, connections);
    let base = size / n;
    let extra = size % n;
    let mut out = Vec::with_capacity(n as usize);
    let mut start = 0;
    for i in 0..n {
        let len = if i < extra { base + 1 } else { base };
        out.push(ByteRange::new(start, start + len));
        start += len;
    }
    out
}

/// Initial plan with the default minimum segment size ([`MIN_SEGMENT_BYTES`]).
pub fn plan_segments_default(size: u64, connections: u32) -> Vec<ByteRange> {
    plan_segments(size, connections, MIN_SEGMENT_BYTES)
}

/// Where to cut `segment` so a second connection can take the upper half of what is left, or
/// `None` if fewer than `min_remainder` bytes remain. The cut is at `next_offset + remaining / 2`,
/// so the original keeps the lower half (and everything already written) and the new segment gets
/// the upper half.
pub fn split_point(segment: &Segment, min_remainder: u64) -> Option<u64> {
    let remaining = segment.remaining();
    if remaining < min_remainder.max(2) {
        return None;
    }
    Some(segment.next_offset() + remaining / 2)
}

/// Work-stealing split. Returns the shrunken original (same index, same `written`, new `end`) and
/// a new pending segment with `new_index` covering the upper half, or `None` when
/// [`split_point`] says the remainder is too small.
pub fn split_segment(
    segment: &Segment,
    new_index: u32,
    min_remainder: u64,
) -> Option<(Segment, Segment)> {
    let mid = split_point(segment, min_remainder)?;
    let lower = Segment {
        end: mid,
        ..segment.clone()
    };
    let upper = Segment::pending(
        segment.download_id,
        new_index,
        ByteRange::new(mid, segment.end),
    );
    Some((lower, upper))
}

/// Among `segments`, the index (into the slice) of the one with the most bytes left that is in
/// one of `states` and can still be split, lowest slice index on ties. This is the segment a
/// freed connection should steal from.
pub fn pick_split_candidate(
    segments: &[Segment],
    states: &[SegmentState],
    min_remainder: u64,
) -> Option<usize> {
    segments
        .iter()
        .enumerate()
        .filter(|(_, s)| states.contains(&s.state) && split_point(s, min_remainder).is_some())
        .max_by(|(ia, a), (ib, b)| a.remaining().cmp(&b.remaining()).then(ib.cmp(ia)))
        .map(|(i, _)| i)
}

/// Checks that `ranges` is a valid plan for `size`: sorted, contiguous, non-overlapping, covering
/// `[0, size)` exactly. Used by the store before trusting rows on disk and by the tests.
pub fn validate_plan(ranges: &[ByteRange], size: u64) -> Result<(), crate::Error> {
    let mut expected_start = 0;
    for (i, r) in ranges.iter().enumerate() {
        if r.start != expected_start {
            return Err(crate::Error::Invalid(format!(
                "segment {i} starts at {} but the previous one ended at {expected_start}",
                r.start
            )));
        }
        if r.end < r.start {
            return Err(crate::Error::Invalid(format!(
                "segment {i} has end < start"
            )));
        }
        expected_start = r.end;
    }
    if expected_start != size {
        return Err(crate::Error::Invalid(format!(
            "segments cover {expected_start} bytes but the download has {size}"
        )));
    }
    if ranges.is_empty() {
        return Err(crate::Error::Invalid(
            "a plan needs at least one segment".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn zero_size_is_one_empty_segment() {
        assert_eq!(plan_segments(0, 8, MIB), vec![ByteRange::new(0, 0)]);
        assert!(validate_plan(&plan_segments(0, 8, MIB), 0).is_ok());
    }

    #[test]
    fn small_files_get_one_segment() {
        assert_eq!(plan_segments(1, 8, MIB), vec![ByteRange::new(0, 1)]);
        assert_eq!(
            plan_segments(MIB - 1, 8, MIB),
            vec![ByteRange::new(0, MIB - 1)]
        );
        assert_eq!(plan_segments(2 * MIB - 1, 8, MIB).len(), 1);
    }

    #[test]
    fn segment_count_follows_the_rule() {
        assert_eq!(plan_segments(2 * MIB, 8, MIB).len(), 2);
        assert_eq!(plan_segments(5 * MIB, 8, MIB).len(), 5);
        assert_eq!(plan_segments(100 * MIB, 8, MIB).len(), 8);
        assert_eq!(plan_segments(100 * MIB, 32, MIB).len(), 32);
        assert_eq!(
            plan_segments(100 * MIB, 0, MIB).len(),
            1,
            "0 connections means 1"
        );
    }

    #[test]
    fn ranges_are_balanced_and_contiguous() {
        let plan = plan_segments(10 * MIB + 3, 4, MIB);
        assert_eq!(plan.len(), 4);
        let lens: Vec<u64> = plan.iter().map(ByteRange::len).collect();
        let (min, max) = (lens.iter().min().unwrap(), lens.iter().max().unwrap());
        assert!(max - min <= 1);
        assert_eq!(plan[0].start, 0);
        assert_eq!(plan[3].end, 10 * MIB + 3);
        assert_eq!(
            plan[0].http_range_value(),
            format!("bytes=0-{}", plan[0].end - 1)
        );
    }

    #[test]
    fn split_respects_two_mib_rule() {
        let id = DownloadId(1);
        let mut s = Segment::pending(id, 0, ByteRange::new(0, 10 * MIB));
        s.state = SegmentState::Active;
        s.written = 3 * MIB;
        // 7 MiB remain -> split at 3 + 3.5 = 6.5 MiB
        let (lower, upper) = split_segment(&s, 5, 2 * MIB).unwrap();
        assert_eq!(lower.index, 0);
        assert_eq!(lower.start, 0);
        assert_eq!(lower.written, 3 * MIB);
        assert_eq!(lower.end, 6 * MIB + MIB / 2);
        assert_eq!(upper.index, 5);
        assert_eq!(upper.start, lower.end);
        assert_eq!(upper.end, 10 * MIB);
        assert_eq!(upper.written, 0);
        assert_eq!(upper.state, SegmentState::Pending);

        s.written = 10 * MIB - 2 * MIB + 1; // 2 MiB - 1 remaining
        assert!(split_segment(&s, 6, 2 * MIB).is_none());
        s.written = 8 * MIB; // exactly 2 MiB remaining
        assert!(split_segment(&s, 6, 2 * MIB).is_some());
    }

    #[test]
    fn pick_candidate_prefers_largest_remaining_then_lowest_index() {
        let id = DownloadId(1);
        let mk = |index, start, end, written, state| Segment {
            download_id: id,
            index,
            start,
            end,
            written,
            state,
        };
        let segs = vec![
            mk(0, 0, 10 * MIB, 9 * MIB, SegmentState::Active), // 1 MiB left: too small
            mk(1, 10 * MIB, 20 * MIB, 2 * MIB, SegmentState::Active), // 8 MiB left
            mk(2, 20 * MIB, 30 * MIB, 2 * MIB, SegmentState::Active), // 8 MiB left (tie -> index 1)
            mk(3, 30 * MIB, 50 * MIB, 0, SegmentState::Done),  // not eligible by state
        ];
        assert_eq!(
            pick_split_candidate(&segs, &[SegmentState::Active], 2 * MIB),
            Some(1)
        );
        assert_eq!(
            pick_split_candidate(&segs, &[SegmentState::Done], 2 * MIB),
            Some(3),
            "state filter is honoured"
        );
        assert_eq!(
            pick_split_candidate(&segs[..1], &[SegmentState::Active], 2 * MIB),
            None
        );
    }

    #[test]
    fn validate_plan_catches_gaps_overlaps_and_short_plans() {
        assert!(validate_plan(&[ByteRange::new(0, 5), ByteRange::new(5, 10)], 10).is_ok());
        assert!(validate_plan(&[ByteRange::new(0, 5), ByteRange::new(6, 10)], 10).is_err());
        assert!(validate_plan(&[ByteRange::new(0, 5), ByteRange::new(4, 10)], 10).is_err());
        assert!(validate_plan(&[ByteRange::new(0, 5)], 10).is_err());
        assert!(validate_plan(&[], 0).is_err());
        assert!(validate_plan(&[ByteRange::new(0, 0)], 0).is_ok());
    }

    #[test]
    fn segment_state_names_round_trip() {
        for s in [
            SegmentState::Pending,
            SegmentState::Active,
            SegmentState::Done,
            SegmentState::Failed,
        ] {
            assert_eq!(SegmentState::from_name(s.name()), Some(s));
        }
        assert_eq!(SegmentState::from_name("nope"), None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        /// Segments cover [0, size) exactly, in order, with no gaps or overlaps.
        #[test]
        fn plan_covers_exactly(size in 0u64..(64 * 1024 * MIB), connections in 0u32..64, min in 1u64..(8 * MIB)) {
            let plan = plan_segments(size, connections, min);
            prop_assert!(validate_plan(&plan, size).is_ok());
            prop_assert_eq!(plan[0].start, 0);
            prop_assert_eq!(plan.last().unwrap().end, size);
            for w in plan.windows(2) {
                prop_assert_eq!(w[0].end, w[1].start);
                prop_assert!(w[0].start < w[0].end);
            }
        }

        /// Count is min(connections, size / min) clamped to >= 1, and sizes are balanced.
        #[test]
        fn plan_count_and_balance(size in 1u64..(64 * 1024 * MIB), connections in 1u32..64, min in 1u64..(8 * MIB)) {
            let plan = plan_segments(size, connections, min);
            let expected = (size / min).clamp(1, u64::from(connections));
            prop_assert_eq!(plan.len() as u64, expected);
            let lens: Vec<u64> = plan.iter().map(ByteRange::len).collect();
            let (lo, hi) = (*lens.iter().min().unwrap(), *lens.iter().max().unwrap());
            prop_assert!(hi - lo <= 1);
        }

        /// Minimum size is respected whenever the file is big enough to have one segment of it.
        #[test]
        fn plan_respects_min_size(size in 1u64..(64 * 1024 * MIB), connections in 1u32..64, min in 1u64..(8 * MIB)) {
            let plan = plan_segments(size, connections, min);
            if size >= min {
                for r in &plan {
                    prop_assert!(r.len() >= min, "range {:?} shorter than min {}", r, min);
                }
            } else {
                prop_assert_eq!(plan.len(), 1);
            }
        }

        /// Splitting keeps the union of the two halves equal to the original range, keeps `written`
        /// on the lower half, hands the upper half a fresh pending segment, and refuses when fewer
        /// than `min_remainder` bytes remain.
        #[test]
        fn split_rule(start in 0u64..(1024 * MIB), len in 0u64..(64 * MIB), written_frac in 0.0f64..=1.0, min_rem in 2u64..(8 * MIB)) {
            let written = ((len as f64) * written_frac) as u64;
            let written = written.min(len);
            let seg = Segment { download_id: DownloadId(9), index: 3, start, end: start + len, written, state: SegmentState::Active };
            let remaining = len - written;
            match split_segment(&seg, 42, min_rem) {
                None => prop_assert!(remaining < min_rem, "refused a split with {} remaining (min {})", remaining, min_rem),
                Some((lower, upper)) => {
                    prop_assert!(remaining >= min_rem);
                    prop_assert_eq!(lower.start, seg.start);
                    prop_assert_eq!(lower.written, seg.written);
                    prop_assert_eq!(lower.index, seg.index);
                    prop_assert_eq!(lower.end, upper.start);
                    prop_assert_eq!(upper.end, seg.end);
                    prop_assert_eq!(upper.written, 0);
                    prop_assert_eq!(upper.index, 42);
                    prop_assert_eq!(upper.state, SegmentState::Pending);
                    // the cut is in the middle of what remains
                    let lower_rem = lower.remaining();
                    let upper_rem = upper.remaining();
                    prop_assert!(lower_rem + upper_rem == remaining);
                    prop_assert!(upper_rem >= lower_rem && upper_rem - lower_rem <= 1);
                    prop_assert!(lower_rem >= 1 && upper_rem >= 1);
                }
            }
        }
    }
}
