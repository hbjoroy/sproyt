use super::DirectActivity;
use chrono::{DateTime, NaiveDate, Utc};

pub(crate) fn empty(now: DateTime<Utc>) -> DirectActivity {
    DirectActivity {
        last_message_at: None,
        score: 0.0,
        calculated_at: now,
    }
}

/// One contribution per UTC active day, independent of message volume.
/// Full history is retained; the contribution halves every 30 elapsed days.
pub(crate) fn add_day(
    activity: &mut DirectActivity,
    day: NaiveDate,
    latest: DateTime<Utc>,
    actor_sent: bool,
    others_sent: bool,
) {
    activity.last_message_at = Some(
        activity
            .last_message_at
            .map_or(latest, |old| old.max(latest)),
    );
    if actor_sent {
        let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let age_days = (activity.calculated_at - start).num_seconds().max(0) as f64 / 86_400.0;
        activity.score += (1.0 + f64::from(others_sent)) * 2.0_f64.powf(-age_days / 30.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_activity_utc_days_and_half_life() {
        let now = DateTime::parse_from_rfc3339("2026-10-28T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut activity = empty(now);
        let old = now - chrono::Duration::days(30);
        add_day(&mut activity, old.date_naive(), old, true, true);
        assert!((activity.score - 1.0).abs() < 1e-10);
        add_day(&mut activity, now.date_naive(), now, false, true);
        assert_eq!(activity.score, 1.0);
        assert_eq!(activity.last_message_at, Some(now));
        add_day(&mut activity, now.date_naive(), now, true, false);
        assert_eq!(activity.score, 2.0);
    }
}
