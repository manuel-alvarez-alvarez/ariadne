//! The once-per-local-day Laya refresh clock.

use std::time::Duration;

use chrono::{DateTime, Local, NaiveTime, TimeZone};

use super::Laya;
use ariadne_store::LayaUpdate;

pub fn start(laya: Laya, every: Duration) {
    tokio::spawn(async move {
        let mut since = Local::now();
        let start_of_day = Local
            .from_local_datetime(&since.date_naive().and_hms_opt(0, 0, 0).expect("midnight"))
            .earliest()
            .expect("local midnight");
        laya.run_schedule(start_of_day, since).await;
        loop {
            tokio::time::sleep(every).await;
            let now = Local::now();
            laya.run_schedule(since, now).await;
            since = now;
        }
    });
}

impl Laya {
    pub async fn run_schedule(&self, since: DateTime<Local>, now: DateTime<Local>) -> bool {
        let Ok(settings) = self.store.laya_settings().await else {
            return false;
        };
        let Some(schedule) = settings.schedule.filter(|_| settings.enabled) else {
            return false;
        };
        let today = now.format("%F").to_string();
        let clock = now.format("%H:%M").to_string();
        let catch_up =
            since.date_naive() == now.date_naive() && schedule.as_str() <= clock.as_str();
        if settings.last_scheduled_refresh.as_deref() == Some(&today)
            || !(due(&schedule, since, now) || catch_up)
        {
            return false;
        }
        if self.install().await.is_none() {
            return false;
        }
        let _ = self
            .store
            .update_laya_settings(LayaUpdate {
                last_scheduled_refresh: Some(Some(today)),
                ..Default::default()
            })
            .await;
        true
    }
}

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
