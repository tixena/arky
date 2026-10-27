//! rkyv adapters for the four chrono types a frontmatter value holds, which
//! rkyv derives over none of. Each archives its value as plain integers.
//!
//! The module exists with the `binary-format` feature. Each adapter goes in an
//! `#[rkyv(with = ...)]` attribute on a field of its chrono type, as arky's archived frontmatter
//! values use them. An archived integer that chrono holds no value for fails to deserialize.

use chrono::{
    DateTime, Datelike as _, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone as _,
    Timelike as _,
};
use rkyv::rancor::{Fallible, Source};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive as _, Archived, Deserialize as _, Place, Resolver, Serialize as _};
use thiserror::Error;

/// Gives `$adapter` rkyv's three traits over `$value`, archived as `$plain`
/// through `$to` and read back through `$from`.
macro_rules! adapter {
    ($adapter:ty, $value:ty, $plain:ty, $to:ident, $from:ident, $name:literal) => {
        impl ArchiveWith<$value> for $adapter {
            type Archived = Archived<$plain>;
            type Resolver = Resolver<$plain>;

            fn resolve_with(field: &$value, resolver: Self::Resolver, out: Place<Self::Archived>) {
                $to(*field).resolve(resolver, out);
            }
        }

        impl<S> SerializeWith<$value, S> for $adapter
        where
            S: Fallible + ?Sized,
        {
            fn serialize_with(
                field: &$value,
                serializer: &mut S,
            ) -> Result<Self::Resolver, S::Error> {
                $to(*field).serialize(serializer)
            }
        }

        impl<D> DeserializeWith<Archived<$plain>, $value, D> for $adapter
        where
            D: Fallible + ?Sized,
            D::Error: Source,
        {
            fn deserialize_with(
                field: &Archived<$plain>,
                deserializer: &mut D,
            ) -> Result<$value, D::Error> {
                let plain: $plain = field.deserialize(deserializer)?;
                $from(plain).ok_or_else(|| D::Error::new(OutOfRange($name)))
            }
        }
    };
}

/// Seconds from midnight, then nanoseconds. A leap second holds a second or
/// more of nanoseconds.
type SecondsOfDay = (u32, u32);

/// A date's days from the common era, then a time's seconds of the day.
type DaysAndSeconds = (i32, SecondsOfDay);

/// The local date and time, then the offset's seconds east of UTC.
type LocalAndOffset = (DaysAndSeconds, i32);

/// A `NaiveDate`, archived as its days from the common era.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AsDays;

/// A `NaiveDateTime`, archived as its date's days and its time's seconds.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AsDaysAndSeconds;

/// A `DateTime<FixedOffset>`, archived as its local date and time and its
/// offset, so that it reads back at the offset it was written with.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AsLocalAndOffset;

/// A `NaiveTime`, archived as its seconds from midnight and nanoseconds.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AsSecondsOfDay;

/// Archived integers that chrono holds no value for.
#[derive(Debug, Error)]
#[error("the archived {0} is out of chrono's range")]
struct OutOfRange(&'static str);

adapter!(AsDays, NaiveDate, i32, days, date, "date");
adapter!(
    AsSecondsOfDay,
    NaiveTime,
    SecondsOfDay,
    seconds_of_day,
    time,
    "time"
);
adapter!(
    AsDaysAndSeconds,
    NaiveDateTime,
    DaysAndSeconds,
    days_and_seconds,
    date_time,
    "date and time"
);
adapter!(
    AsLocalAndOffset,
    DateTime<FixedOffset>,
    LocalAndOffset,
    local_and_offset,
    offset_date_time,
    "date and time with an offset"
);

fn days(date: NaiveDate) -> i32 {
    date.num_days_from_ce()
}

const fn date(count: i32) -> Option<NaiveDate> {
    NaiveDate::from_num_days_from_ce_opt(count)
}

fn seconds_of_day(time: NaiveTime) -> SecondsOfDay {
    (time.num_seconds_from_midnight(), time.nanosecond())
}

const fn time((seconds, nanoseconds): SecondsOfDay) -> Option<NaiveTime> {
    NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanoseconds)
}

fn days_and_seconds(date_time: NaiveDateTime) -> DaysAndSeconds {
    (days(date_time.date()), seconds_of_day(date_time.time()))
}

fn date_time((day, seconds): DaysAndSeconds) -> Option<NaiveDateTime> {
    Some(date(day)?.and_time(time(seconds)?))
}

fn local_and_offset(date_time: DateTime<FixedOffset>) -> LocalAndOffset {
    (
        days_and_seconds(date_time.naive_local()),
        date_time.offset().local_minus_utc(),
    )
}

fn offset_date_time((local, offset): LocalAndOffset) -> Option<DateTime<FixedOffset>> {
    FixedOffset::east_opt(offset)?
        .from_local_datetime(&date_time(local)?)
        .single()
}
