use chrono::{
    DateTime, Datelike, FixedOffset, Local, NaiveDate, Offset, TimeZone, Timelike, Utc,
};
use serde::{Deserialize, Deserializer, Serializer};

pub fn local_offset() -> FixedOffset {
    Local::now().offset().fix()
}

pub fn local_date(timestamp: DateTime<Utc>, offset: FixedOffset) -> NaiveDate {
    timestamp.with_timezone(&offset).date_naive()
}

pub fn local_start_of_day_utc(date: NaiveDate, offset: FixedOffset) -> DateTime<Utc> {
    offset
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

pub fn local_end_of_day_utc(date: NaiveDate, offset: FixedOffset) -> DateTime<Utc> {
    offset
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 23, 59, 59)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

pub fn local_day_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn local_hour_bucket_key(date: NaiveDate, hour: u32, offset: FixedOffset) -> String {
    offset
        .with_ymd_and_hms(date.year(), date.month(), date.day(), hour, 0, 0)
        .single()
        .unwrap()
        .format("%Y-%m-%dT%H:00:00%:z")
        .to_string()
}

pub fn local_bucket_hour(timestamp: DateTime<Utc>, step_hours: u32, offset: FixedOffset) -> u32 {
    let hour = timestamp.with_timezone(&offset).hour();
    hour - (hour % step_hours)
}

pub fn local_bucket_key(
    timestamp: DateTime<Utc>,
    step_hours: Option<u32>,
    offset: FixedOffset,
) -> String {
    let date = local_date(timestamp, offset);
    match step_hours {
        Some(step_hours) => local_hour_bucket_key(date, local_bucket_hour(timestamp, step_hours, offset), offset),
        None => local_day_key(date),
    }
}

pub fn month_end_date(date: NaiveDate) -> NaiveDate {
    let (next_year, next_month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap_or(date) - chrono::Duration::days(1)
}

pub mod local_datetime_serde {
    use super::*;
    use serde::de::Error;

    pub fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.with_timezone(&Local).to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&value)
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(D::Error::custom)
    }

    pub mod option {
        use super::*;

        pub fn serialize<S>(
            value: &Option<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match value {
                Some(timestamp) => serializer.serialize_some(&timestamp.with_timezone(&Local).to_rfc3339()),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(
            deserializer: D,
        ) -> Result<Option<DateTime<Utc>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let value = Option::<String>::deserialize(deserializer)?;
            value
                .map(|value| {
                    DateTime::parse_from_rfc3339(&value)
                        .map(|timestamp| timestamp.with_timezone(&Utc))
                        .map_err(serde::de::Error::custom)
                })
                .transpose()
        }
    }
}
