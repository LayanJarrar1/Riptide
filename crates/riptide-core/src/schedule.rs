//! Queue schedules: pure logic, no clock, no IO.
//!
//! A [`Schedule`] says when a queue is *allowed to run*: a daily window (start and stop time,
//! local wall-clock), the days of the week it applies to, and an optional one-shot "start at".
//! The engine asks two questions, both answered here from a caller-supplied `now`:
//!
//! * [`Schedule::active_at`] — should the queue be running at this instant?
//! * [`Schedule::next_transition`] — when does that answer next change?
//!
//! Times are [`LocalDateTime`]s: wall-clock seconds as if the local time zone were UTC. The
//! engine converts the system time once (see [`Clock`]); DST shifts are therefore absorbed by
//! re-reading the clock, never by arithmetic here. Tests drive a [`ManualClock`].
//!
//! **A sleeping PC is not woken (v1).** Schedules are evaluated by a timer inside the running
//! engine; if the machine is asleep or off at the start time, the queue starts when the engine
//! next runs and finds the window open (or the one-shot start time passed).

use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Seconds in a day.
pub const SECS_PER_DAY: i64 = 86_400;
/// Minutes in a day.
pub const MINUTES_PER_DAY: u16 = 1_440;
/// Start of the "only at night" preset (23:00).
pub const NIGHT_START_MIN: u16 = 23 * 60;
/// End of the "only at night" preset (07:00).
pub const NIGHT_STOP_MIN: u16 = 7 * 60;

/// Wall-clock date and time in the user's time zone, as seconds since 1970-01-01T00:00:00
/// *local* time. Not a real instant: two zones' values are not comparable.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalDateTime(#[cfg_attr(feature = "ts", ts(type = "number"))] pub i64);

impl LocalDateTime {
    /// From a calendar date and time of day; `None` if any part is out of range.
    pub fn from_ymd_hms(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
    ) -> Option<Self> {
        if !(1..=12).contains(&month) || hour > 23 || min > 59 || sec > 59 {
            return None;
        }
        let days = days_from_civil(i64::from(year), i64::from(month), 1);
        let next = if month == 12 {
            days_from_civil(i64::from(year) + 1, 1, 1)
        } else {
            days_from_civil(i64::from(year), i64::from(month) + 1, 1)
        };
        if day < 1 || i64::from(day) > next - days {
            return None;
        }
        let days = days + i64::from(day) - 1;
        Some(LocalDateTime(
            days * SECS_PER_DAY + i64::from(hour * 3600 + min * 60 + sec),
        ))
    }

    /// `(year, month, day, hour, minute, second)`.
    pub fn to_ymd_hms(self) -> (i32, u32, u32, u32, u32, u32) {
        let days = self.0.div_euclid(SECS_PER_DAY);
        let rem = self.0.rem_euclid(SECS_PER_DAY);
        let (y, m, d) = civil_from_days(days);
        (
            y as i32,
            m as u32,
            d as u32,
            (rem / 3600) as u32,
            (rem % 3600 / 60) as u32,
            (rem % 60) as u32,
        )
    }

    /// Day of the week.
    pub fn weekday(self) -> Weekday {
        // 1970-01-01 was a Thursday
        Weekday::from_index((self.0.div_euclid(SECS_PER_DAY) + 3).rem_euclid(7) as u8)
    }

    /// Minutes since local midnight, `0..1440`.
    pub fn minute_of_day(self) -> u16 {
        (self.0.rem_euclid(SECS_PER_DAY) / 60) as u16
    }

    /// Local midnight of the same day.
    pub fn start_of_day(self) -> LocalDateTime {
        LocalDateTime(self.0.div_euclid(SECS_PER_DAY) * SECS_PER_DAY)
    }

    /// `self` moved by `secs` (may be negative).
    pub fn plus_secs(self, secs: i64) -> LocalDateTime {
        LocalDateTime(self.0.saturating_add(secs))
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Day of the week, Monday first.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    /// `0` = Monday … `6` = Sunday.
    pub const fn index(self) -> u8 {
        self as u8
    }

    const fn from_index(i: u8) -> Weekday {
        match i % 7 {
            0 => Weekday::Mon,
            1 => Weekday::Tue,
            2 => Weekday::Wed,
            3 => Weekday::Thu,
            4 => Weekday::Fri,
            5 => Weekday::Sat,
            _ => Weekday::Sun,
        }
    }

    /// The day before.
    pub const fn prev(self) -> Weekday {
        Weekday::from_index(self.index() + 6)
    }
}

/// A set of weekdays as a bit mask (bit 0 = Monday … bit 6 = Sunday).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DaySet(#[cfg_attr(feature = "ts", ts(type = "number"))] pub u8);

impl DaySet {
    /// Every day.
    pub const ALL: DaySet = DaySet(0b0111_1111);
    /// No day (invalid in a schedule that has a window).
    pub const NONE: DaySet = DaySet(0);
    /// Monday to Friday.
    pub const WEEKDAYS: DaySet = DaySet(0b0001_1111);

    /// The set containing exactly `days`.
    pub fn of(days: &[Weekday]) -> DaySet {
        DaySet(days.iter().fold(0, |m, d| m | 1 << d.index()))
    }

    /// Whether `day` is in the set.
    pub const fn contains(self, day: Weekday) -> bool {
        self.0 & (1 << day.index()) != 0
    }

    /// No day set.
    pub const fn is_empty(self) -> bool {
        self.0 & Self::ALL.0 == 0
    }
}

impl Default for DaySet {
    fn default() -> Self {
        DaySet::ALL
    }
}

/// A daily window in minutes after local midnight. `start_min > stop_min` wraps past midnight
/// (23:00–07:00); the window belongs to the day it starts on.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Queue starts at this minute of the day (`0..1440`).
    pub start_min: u16,
    /// Queue stops at this minute of the day (`0..1440`), exclusive.
    pub stop_min: u16,
}

impl Window {
    /// 23:00–07:00.
    pub const NIGHT: Window = Window {
        start_min: NIGHT_START_MIN,
        stop_min: NIGHT_STOP_MIN,
    };
}

/// When a queue may run. The default schedule is *unscheduled*: the queue is governed only by
/// its manual start/stop switch.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    /// Daily start/stop; `None` = all day on the selected [`Schedule::days`].
    pub window: Option<Window>,
    /// Days the window applies to (a wrapping window counts from the day it starts).
    pub days: DaySet,
    /// One-shot: nothing runs before this moment. The engine clears it once it has fired.
    pub start_at: Option<LocalDateTime>,
}

impl Schedule {
    /// The "only at night" preset: 23:00–07:00, every day.
    pub const fn only_at_night() -> Schedule {
        Schedule {
            window: Some(Window::NIGHT),
            days: DaySet::ALL,
            start_at: None,
        }
    }

    /// A one-shot "start at" with no daily window.
    pub const fn start_once_at(at: LocalDateTime) -> Schedule {
        Schedule {
            window: None,
            days: DaySet::ALL,
            start_at: Some(at),
        }
    }

    /// `true` when the schedule imposes nothing (no window, every day, no one-shot).
    pub fn is_unscheduled(&self) -> bool {
        self.window.is_none() && self.days == DaySet::ALL && self.start_at.is_none()
    }

    /// Checks the fields: minutes inside the day, `start != stop`, at least one day.
    pub fn validate(&self) -> Result<(), Error> {
        if let Some(w) = self.window {
            if w.start_min >= MINUTES_PER_DAY || w.stop_min >= MINUTES_PER_DAY {
                return Err(Error::Invalid(
                    "schedule time must be within the day".into(),
                ));
            }
            if w.start_min == w.stop_min {
                return Err(Error::Invalid(
                    "schedule start and stop time must differ".into(),
                ));
            }
        }
        if self.days.is_empty() {
            return Err(Error::Invalid("schedule needs at least one day".into()));
        }
        Ok(())
    }

    /// Whether the queue should be running at `now`.
    pub fn active_at(&self, now: LocalDateTime) -> bool {
        if self.start_at.is_some_and(|t| now < t) {
            return false;
        }
        let day = now.weekday();
        match self.window {
            None => self.days.contains(day),
            Some(Window {
                start_min,
                stop_min,
            }) => {
                let m = now.minute_of_day();
                if start_min < stop_min {
                    self.days.contains(day) && m >= start_min && m < stop_min
                } else {
                    (m >= start_min && self.days.contains(day))
                        || (m < stop_min && self.days.contains(day.prev()))
                }
            }
        }
    }

    /// The first instant after `now` at which [`Schedule::active_at`] changes, looking at most
    /// a week ahead (a weekly pattern repeats, so `None` means it never changes).
    pub fn next_transition(&self, now: LocalDateTime) -> Option<LocalDateTime> {
        let mut candidates = Vec::with_capacity(24);
        if let Some(t) = self.start_at {
            candidates.push(t);
        }
        let today = now.start_of_day();
        for offset in -1..=8_i64 {
            let base = today.plus_secs(offset * SECS_PER_DAY);
            match self.window {
                Some(w) => {
                    candidates.push(base.plus_secs(i64::from(w.start_min) * 60));
                    candidates.push(base.plus_secs(i64::from(w.stop_min) * 60));
                }
                None => candidates.push(base),
            }
        }
        candidates.retain(|c| *c > now);
        candidates.sort_unstable();
        candidates.dedup();
        candidates
            .into_iter()
            .find(|c| self.active_at(*c) != self.active_at(c.plus_secs(-1)))
    }

    /// The one-shot start time has been reached.
    pub fn start_due(&self, now: LocalDateTime) -> bool {
        self.start_at.is_some_and(|t| now >= t)
    }
}

/// Source of local wall-clock time. The engine uses the system clock; tests use
/// [`ManualClock`].
pub trait Clock: std::fmt::Debug + Send + Sync {
    /// Now, in the user's local time.
    fn now(&self) -> LocalDateTime;
}

/// A clock that only moves when told to (tests, simulations).
#[derive(Debug)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    /// Starts at `at`.
    pub fn new(at: LocalDateTime) -> Self {
        ManualClock(AtomicI64::new(at.0))
    }

    /// Jumps to `at`.
    pub fn set(&self, at: LocalDateTime) {
        self.0.store(at.0, Ordering::SeqCst);
    }

    /// Moves forward by `secs`.
    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime(self.0.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> LocalDateTime {
        LocalDateTime::from_ymd_hms(y, mo, d, h, mi, 0).unwrap()
    }

    // 2026-10-19 is a Monday.
    const MON: (i32, u32, u32) = (2026, 10, 19);

    fn mon(h: u32, mi: u32) -> LocalDateTime {
        at(MON.0, MON.1, MON.2, h, mi)
    }

    #[test]
    fn calendar_round_trip_and_weekday() {
        assert_eq!(mon(0, 0).weekday(), Weekday::Mon);
        assert_eq!(
            mon(0, 0).plus_secs(SECS_PER_DAY * 6).weekday(),
            Weekday::Sun
        );
        assert_eq!(LocalDateTime(0).weekday(), Weekday::Thu);
        assert_eq!(mon(13, 45).to_ymd_hms(), (2026, 10, 19, 13, 45, 0));
        assert_eq!(mon(13, 45).minute_of_day(), 13 * 60 + 45);
        let leap = LocalDateTime::from_ymd_hms(2028, 2, 29, 0, 0, 0).unwrap();
        assert_eq!(leap.to_ymd_hms(), (2028, 2, 29, 0, 0, 0));
        assert!(LocalDateTime::from_ymd_hms(2027, 2, 29, 0, 0, 0).is_none());
        assert!(LocalDateTime::from_ymd_hms(2027, 13, 1, 0, 0, 0).is_none());
    }

    #[test]
    fn unscheduled_is_always_active_and_never_changes() {
        let s = Schedule::default();
        assert!(s.is_unscheduled());
        assert!(s.active_at(mon(3, 0)));
        assert_eq!(s.next_transition(mon(3, 0)), None);
    }

    #[test]
    fn daytime_window() {
        let s = Schedule {
            window: Some(Window {
                start_min: 9 * 60,
                stop_min: 17 * 60,
            }),
            ..Schedule::default()
        };
        assert!(!s.active_at(mon(8, 59)));
        assert!(s.active_at(mon(9, 0)));
        assert!(s.active_at(mon(16, 59)));
        assert!(!s.active_at(mon(17, 0)));
        assert_eq!(s.next_transition(mon(8, 0)), Some(mon(9, 0)));
        assert_eq!(s.next_transition(mon(9, 0)), Some(mon(17, 0)));
        // after the stop, the next start is tomorrow morning
        assert_eq!(
            s.next_transition(mon(17, 0)),
            Some(mon(9, 0).plus_secs(SECS_PER_DAY))
        );
    }

    #[test]
    fn night_preset_wraps_midnight() {
        let s = Schedule::only_at_night();
        assert!(!s.is_unscheduled());
        assert!(!s.active_at(mon(22, 59)));
        assert!(s.active_at(mon(23, 0)));
        assert!(s.active_at(mon(23, 59)));
        assert!(s.active_at(mon(0, 0).plus_secs(SECS_PER_DAY)));
        assert!(s.active_at(mon(6, 59)));
        assert!(!s.active_at(mon(7, 0)));
        assert_eq!(s.next_transition(mon(12, 0)), Some(mon(23, 0)));
        assert_eq!(
            s.next_transition(mon(23, 30)),
            Some(mon(7, 0).plus_secs(SECS_PER_DAY))
        );
    }

    #[test]
    fn wrapping_window_belongs_to_the_day_it_starts() {
        // Friday night only: Fri 23:00 -> Sat 07:00
        let s = Schedule {
            window: Some(Window::NIGHT),
            days: DaySet::of(&[Weekday::Fri]),
            start_at: None,
        };
        let fri = mon(0, 0).plus_secs(4 * SECS_PER_DAY);
        let sat = fri.plus_secs(SECS_PER_DAY);
        assert!(!s.active_at(fri.plus_secs(22 * 3600)));
        assert!(s.active_at(fri.plus_secs(23 * 3600)));
        assert!(s.active_at(sat.plus_secs(6 * 3600)));
        assert!(!s.active_at(sat.plus_secs(7 * 3600)));
        // Saturday 23:00 does not start a window; Monday 03:00 is not inside one
        assert!(!s.active_at(sat.plus_secs(23 * 3600)));
        assert!(!s.active_at(mon(3, 0)));
        // from Monday the next edge is Friday 23:00
        assert_eq!(
            s.next_transition(mon(12, 0)),
            Some(fri.plus_secs(23 * 3600))
        );
    }

    #[test]
    fn days_of_week_gate_a_daytime_window() {
        let s = Schedule {
            window: Some(Window {
                start_min: 60,
                stop_min: 5 * 60,
            }),
            days: DaySet::WEEKDAYS,
            start_at: None,
        };
        let sat = mon(0, 0).plus_secs(5 * SECS_PER_DAY);
        assert!(s.active_at(mon(2, 0)));
        assert!(!s.active_at(sat.plus_secs(2 * 3600)));
        // Friday 05:00 stop, then nothing until Monday 01:00
        let fri_stop = mon(5, 0).plus_secs(4 * SECS_PER_DAY);
        assert_eq!(s.next_transition(fri_stop.plus_secs(-60)), Some(fri_stop));
        assert_eq!(
            s.next_transition(fri_stop),
            Some(mon(1, 0).plus_secs(7 * SECS_PER_DAY))
        );
    }

    #[test]
    fn days_without_a_window_gate_whole_days() {
        let s = Schedule {
            window: None,
            days: DaySet::of(&[Weekday::Sat, Weekday::Sun]),
            start_at: None,
        };
        let sat = mon(0, 0).plus_secs(5 * SECS_PER_DAY);
        assert!(!s.active_at(mon(12, 0)));
        assert!(s.active_at(sat.plus_secs(1)));
        assert_eq!(s.next_transition(mon(12, 0)), Some(sat));
        assert_eq!(
            s.next_transition(sat),
            Some(sat.plus_secs(2 * SECS_PER_DAY))
        );
    }

    #[test]
    fn one_shot_start_gates_everything_until_it_fires() {
        let s = Schedule::start_once_at(mon(14, 30));
        assert!(!s.active_at(mon(14, 29)));
        assert!(s.active_at(mon(14, 30)));
        assert!(s.start_due(mon(14, 30)));
        assert!(!s.start_due(mon(14, 29)));
        assert_eq!(s.next_transition(mon(9, 0)), Some(mon(14, 30)));
        assert_eq!(s.next_transition(mon(14, 30)), None);
        // combined with a window, the window still applies afterwards
        let both = Schedule {
            window: Some(Window::NIGHT),
            start_at: Some(mon(12, 0)),
            ..Schedule::default()
        };
        assert!(!both.active_at(mon(23, 30).plus_secs(-SECS_PER_DAY)));
        assert!(!both.active_at(mon(13, 0)));
        assert!(both.active_at(mon(23, 30)));
    }

    #[test]
    fn manual_clock_drives_the_questions() {
        let clock = ManualClock::new(mon(22, 0));
        let s = Schedule::only_at_night();
        assert!(!s.active_at(clock.now()));
        let next = s.next_transition(clock.now()).unwrap();
        clock.set(next);
        assert!(s.active_at(clock.now()));
        clock.advance(8 * 3600);
        assert!(!s.active_at(clock.now()));
    }

    #[test]
    fn validation() {
        assert!(Schedule::default().validate().is_ok());
        assert!(Schedule::only_at_night().validate().is_ok());
        let bad = |s: Schedule| assert!(s.validate().is_err(), "{s:?}");
        bad(Schedule {
            window: Some(Window {
                start_min: 60,
                stop_min: 60,
            }),
            ..Schedule::default()
        });
        bad(Schedule {
            window: Some(Window {
                start_min: 1440,
                stop_min: 5,
            }),
            ..Schedule::default()
        });
        bad(Schedule {
            days: DaySet::NONE,
            ..Schedule::default()
        });
    }

    #[test]
    fn json_shape() {
        let s = Schedule::only_at_night();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Schedule>(&json).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<Schedule>("{}").unwrap(),
            Schedule::default()
        );
    }

    fn arb_schedule() -> impl Strategy<Value = Schedule> {
        (
            proptest::option::of((0u16..1440, 0u16..1440).prop_filter("distinct", |(a, b)| a != b)),
            1u8..128,
            proptest::option::of(0i64..(10 * SECS_PER_DAY)),
        )
            .prop_map(|(w, days, start)| Schedule {
                window: w.map(|(start_min, stop_min)| Window {
                    start_min,
                    stop_min,
                }),
                days: DaySet(days),
                start_at: start.map(|s| LocalDateTime(1_800_000_000 + s)),
            })
    }

    proptest! {
        /// The answer is constant between `now` and the next transition, and flips there.
        #[test]
        fn next_transition_is_the_first_change(
            s in arb_schedule(),
            now in 1_790_000_000i64..1_820_000_000,
        ) {
            let now = LocalDateTime(now);
            if let Some(t) = s.next_transition(now) {
                prop_assert!(t > now);
                prop_assert_ne!(s.active_at(t), s.active_at(t.plus_secs(-1)));
                prop_assert_eq!(s.active_at(t.plus_secs(-1)), s.active_at(now));
                // sampled instants before the edge never differ from now
                for k in 1..=10i64 {
                    let probe = LocalDateTime(now.0 + (t.0 - now.0) * k / 11);
                    prop_assert_eq!(s.active_at(probe), s.active_at(now));
                }
            } else {
                // never changes within a week
                for k in 1..=40i64 {
                    prop_assert_eq!(s.active_at(now.plus_secs(k * 4 * 3600)), s.active_at(now));
                }
            }
        }
    }
}
