//! Instants as files and tools write them: RFC 3339 in this host's offset.
//!
//! An instant is a [`jiff::Timestamp`]. It is written with the offset this
//! host's time zone has at that instant (`2026-10-28T09:00:00+09:00`), so a
//! file reads naturally and a release still on chrono reads it. Read back, the
//! offset only places the instant: days are the viewer's, in this host's time
//! zone.
//!
//! A value without an offset — written by hand, or by an agent — is a time in
//! this host's time zone; a time a DST change skips or repeats resolves as
//! jiff's `compatible` (Temporal's) does.

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};

/// Now.
pub fn now() -> Timestamp {
    Timestamp::now()
}

/// `timestamp` in this host's time zone.
pub fn local(timestamp: Timestamp) -> ZonedDateTime {
    timestamp.to_zoned_datetime(TimeZone::system())
}

/// The day `timestamp` falls on in this host's time zone.
pub fn local_date(timestamp: Timestamp) -> Date {
    local(timestamp).date()
}

/// The instant a wall-clock time in this host's time zone names.
pub fn from_local(datetime: DateTime) -> Result<Timestamp, jiff::Error> {
    TimeZone::system().to_timestamp(datetime)
}

/// `timestamp` as RFC 3339 in this host's offset at that instant.
pub fn format(timestamp: Timestamp) -> String {
    let offset = TimeZone::system().to_offset(timestamp);
    timestamp.display_with_offset(offset).to_string()
}

/// RFC 3339 with an offset or `Z` (a `[Area/City]` annotation is allowed), or
/// a date and time without one, read in this host's time zone.
pub fn parse(text: &str) -> Result<Timestamp, jiff::Error> {
    let text = text.trim();
    match text.parse::<Timestamp>() {
        Ok(timestamp) => Ok(timestamp),
        Err(err) => match text.parse::<DateTime>() {
            Ok(datetime) => from_local(datetime),
            Err(_) => Err(err),
        },
    }
}

/// `#[serde(with = "…::rfc3339")]` for a `Timestamp`.
pub mod rfc3339 {
    use jiff::Timestamp;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(timestamp: &Timestamp, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::format(*timestamp))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Timestamp, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        super::parse(&text).map_err(serde::de::Error::custom)
    }

    /// The same for an `Option<Timestamp>`.
    pub mod option {
        use jiff::Timestamp;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            timestamp: &Option<Timestamp>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            match timestamp {
                Some(t) => super::serialize(t, s),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Timestamp>, D::Error> {
            let text = <Option<std::borrow::Cow<'de, str>>>::deserialize(d)?;
            text.map(|t| super::super::parse(&t).map_err(serde::de::Error::custom))
                .transpose()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_chrono_wrote() {
        for (text, utc) in [
            ("2026-10-28T09:00:00+09:00", "2026-10-28T00:00:00Z"),
            (
                "2026-10-28T09:00:00.123456789+09:00",
                "2026-10-28T00:00:00.123456789Z",
            ),
            ("2026-10-28T00:00:00+00:00", "2026-10-28T00:00:00Z"),
            ("2026-10-28T00:00:00Z", "2026-10-28T00:00:00Z"),
            ("2026-10-27T19:30:00-04:30", "2026-10-28T00:00:00Z"),
            (
                "2026-10-28T09:00:00+09:00[Asia/Tokyo]",
                "2026-10-28T00:00:00Z",
            ),
        ] {
            assert_eq!(
                parse(text).unwrap(),
                utc.parse::<Timestamp>().unwrap(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_time_without_an_offset_is_local() {
        let datetime = jiff::civil::date(2026, 10, 28).at(9, 0, 0, 0);
        assert_eq!(
            parse("2026-10-28T09:00:00").unwrap(),
            from_local(datetime).unwrap()
        );
        assert_eq!(
            parse("2026-10-28T09:00").unwrap(),
            from_local(datetime).unwrap()
        );
        assert!(parse("tomorrow").is_err());
    }

    #[test]
    fn writes_an_offset_and_reads_it_back() {
        let t: Timestamp = "2026-10-28T00:00:00.5Z".parse().unwrap();
        assert_eq!(parse(&format(t)).unwrap(), t);
    }
}
