//! The daily refresh: the install again, once a day at the `HH:MM` the
//! settings name, in local time.
//!
//! A clock the daemon looks at every `Timeouts::laya_schedule_poll`. A tick
//! refreshes when the scheduled minute fell between the tick before it and
//! this one, so each day's refresh runs once however often the clock is read.
//! A daemon that was not running at that minute does not catch up when it
//! starts: the next day's minute is the next refresh.

use std::time::Duration;

use chrono::{DateTime, Local, NaiveTime, TimeZone};

use super::Laya;

/// Read the clock every `every`, and refresh when the scheduled minute has
/// come round. Runs for the life of the daemon.
pub fn start(laya: Laya, every: Duration) {
    tokio::spawn(async move {
        let mut since = Local::now();
        loop {
            tokio::time::sleep(every).await;
            let now = Local::now();
            laya.run_schedule(since, now).await;
            since = now;
        }
    });
}

impl Laya {
    /// One tick of the daily refresh: start an install where the scheduled
    /// minute fell after `since` and at or before `now`, Laya is on, and no
    /// install is running. Whether one started is the answer.
    ///
    /// The clock is a parameter so that a test says what time it is.
    pub async fn run_schedule(&self, since: DateTime<Local>, now: DateTime<Local>) -> bool {
        let row = match self.store.laya_settings().await {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(error = %error, "reading the Laya schedule failed");
                return false;
            }
        };
        let Some(schedule) = row.schedule.filter(|_| row.enabled) else {
            return false;
        };
        if !due(&schedule, since, now) {
            return false;
        }
        let started = self.install().await.is_some();
        if !started {
            tracing::info!("the daily Laya refresh is skipped: an install is already running");
        }
        started
    }
}

/// Whether an `HH:MM` of local time lies after `since` and at or before
/// `now`, on any day between the two.
///
/// A minute the clock skips — the hour a daylight-saving change jumps over —
/// is not a time on that day, and nothing runs on it. A minute the clock
/// passes twice counts at its first passing.
fn due(schedule: &str, since: DateTime<Local>, now: DateTime<Local>) -> bool {
    let Ok(at) = NaiveTime::parse_from_str(schedule, "%H:%M") else {
        return false;
    };
    let mut day = since.date_naive();
    while day <= now.date_naive() {
        if let Some(slot) = Local.from_local_datetime(&day.and_time(at)).earliest()
            && since < slot
            && slot <= now
        {
            return true;
        }
        let Some(next) = day.succ_opt() else {
            return false;
        };
        day = next;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local time on a January day, when no daylight-saving change is near.
    fn at(day: u32, time: &str) -> DateTime<Local> {
        let naive = chrono::NaiveDate::from_ymd_opt(2026, 1, day)
            .unwrap()
            .and_time(NaiveTime::parse_from_str(time, "%H:%M:%S").unwrap());
        Local.from_local_datetime(&naive).earliest().unwrap()
    }

    /// The minute is due on the tick that passes it, and on no other: the
    /// tick before it and the tick after it say no.
    #[test]
    fn the_minute_is_due_on_the_tick_that_passes_it_and_no_other() {
        assert!(due("03:30", at(10, "03:29:40"), at(10, "03:30:10")));
        // Exactly on the minute counts, once: the tick that ends on it.
        assert!(due("03:30", at(10, "03:29:40"), at(10, "03:30:00")));
        assert!(!due("03:30", at(10, "03:30:00"), at(10, "03:30:30")));

        assert!(!due("03:30", at(10, "03:29:00"), at(10, "03:29:30")));
        assert!(!due("03:30", at(10, "03:30:10"), at(10, "03:30:40")));
    }

    /// Midnight is a minute of the new day, and a tick that runs over it
    /// passes it.
    #[test]
    fn a_tick_over_midnight_passes_the_minute_of_the_new_day() {
        assert!(due("00:00", at(10, "23:59:50"), at(11, "00:00:20")));
        assert!(!due("23:59", at(10, "23:59:50"), at(11, "00:00:20")));
    }

    /// A daemon that starts after the minute has passed does not catch up:
    /// its first tick begins where it started.
    #[test]
    fn a_minute_before_the_first_tick_is_not_caught_up() {
        assert!(!due("03:30", at(10, "09:00:00"), at(10, "09:00:30")));
    }

    /// A schedule that is not a time runs nothing.
    #[test]
    fn a_schedule_that_is_not_a_time_is_never_due() {
        assert!(!due("25:00", at(10, "00:00:00"), at(11, "00:00:00")));
        assert!(!due("", at(10, "00:00:00"), at(11, "00:00:00")));
    }
}
