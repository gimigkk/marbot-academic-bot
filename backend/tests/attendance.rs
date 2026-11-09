use marbot::attendance::{compare, parse_snapshot, resolve_student, ReminderAction};
use marbot::classifier::classify_message;
use marbot::models::{BotCommand, MessageType};
use serde_json::json;

#[test]
fn polling_uses_wib_class_hours_and_lunch_break() {
    use marbot::attendance::poll_interval_seconds;
    for (time, expected) in [
        ("07:59:59", 3600),
        ("08:00:00", 900),
        ("11:59:59", 900),
        ("12:00:00", 3600),
        ("12:59:59", 3600),
        ("13:00:00", 900),
        ("16:59:59", 900),
        ("17:00:00", 3600),
    ] {
        let now = chrono::DateTime::parse_from_rfc3339(&format!("2026-11-09T{time}+07:00"))
            .unwrap()
            .to_utc();
        assert_eq!(poll_interval_seconds(now), expected, "{time} WIB");
    }
}

#[test]
fn nim_slots_are_stable_spread_out_and_adapt_at_boundaries() {
    use marbot::attendance::{next_poll_at, poll_interval_seconds};
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-09T09:00:00+07:00")
        .unwrap()
        .to_utc();
    let next = next_poll_at(377902, now);
    assert!(next > now && next <= now + chrono::Duration::minutes(15));
    assert_eq!(next, next_poll_at(377902, now));
    assert_eq!(
        next_poll_at(377902, next) - next,
        chrono::Duration::minutes(15)
    );
    let night = chrono::DateTime::parse_from_rfc3339("2026-11-09T19:00:00+07:00").unwrap().to_utc();
    let next = next_poll_at(377902, night);
    assert_eq!(next_poll_at(377902, next) - next, chrono::Duration::hours(1));
    let slots: std::collections::BTreeSet<_> = (1..=100).map(|id| next_poll_at(id, now)).collect();
    assert!(slots.len() > 50);
    for time in ["07:59:59", "11:59:59", "12:59:59", "16:59:59", "23:59:59"] {
        let now = chrono::DateTime::parse_from_rfc3339(&format!("2026-11-09T{time}+07:00"))
            .unwrap()
            .to_utc();
        let next = next_poll_at(377902, now);
        assert!(next > now && next <= now + chrono::Duration::hours(1));
        if time == "07:59:59" || time == "12:59:59" {
            assert_eq!(poll_interval_seconds(next), 900);
            assert!(next - now <= chrono::Duration::minutes(15));
        }
    }
}

fn recap(missed: u32, parallel: &str) -> serde_json::Value {
    json!([{"KodeMk":"TEST101","NamaMk":"Test Course","DetailRekap":[{
        "JenisKelas":"K","KelasParalel":parallel,"JumlahHadir":5,
        "JumlahTidakHadir":missed,"JumlahPertemuan":5+missed
    }]}])
}

#[test]
fn first_snapshot_and_unchanged_counts_are_silent() {
    let current = parse_snapshot(recap(2, "1")).unwrap();
    assert!(compare(&Default::default(), &current).is_empty());
    assert!(compare(&current, &current).is_empty());
}

#[test]
fn each_increase_notifies_once_and_corrections_become_the_next_baseline() {
    let baseline = parse_snapshot(recap(1, "1")).unwrap();
    let increased = parse_snapshot(recap(3, "1")).unwrap();
    let changes = compare(&baseline, &increased);
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].previous, changes[0].current), (1, 3));
    assert!(compare(&increased, &increased).is_empty());
    let corrected = parse_snapshot(recap(2, "1")).unwrap();
    assert!(compare(&increased, &corrected).is_empty());
    assert_eq!(compare(&corrected, &increased).len(), 1);
}

#[test]
fn new_classes_and_empty_responses_do_not_invent_missed_meetings() {
    let previous = parse_snapshot(recap(1, "1")).unwrap();
    let new_class = parse_snapshot(recap(4, "2")).unwrap();
    assert!(compare(&previous, &new_class).is_empty());
    assert!(parse_snapshot(json!([])).unwrap().is_empty());
    assert!(parse_snapshot(json!({"Error":"unavailable"})).is_err());
    assert!(parse_snapshot(json!([{"KodeMk":"TEST101"}])).is_err());
    let mut invalid = recap(1, "1");
    invalid[0]["DetailRekap"][0]["JumlahPertemuan"] = json!(0);
    assert!(parse_snapshot(invalid).is_err());
}

#[test]
fn rejects_duplicate_class_keys() {
    let mut value = recap(1, "1");
    let duplicate = value[0].clone();
    value.as_array_mut().unwrap().push(duplicate);
    assert!(parse_snapshot(value).is_err());
}

#[test]
fn a_lower_meeting_total_is_a_silent_reset() {
    let mut old = recap(1, "1");
    old[0]["DetailRekap"][0]["JumlahHadir"] = json!(19);
    old[0]["DetailRekap"][0]["JumlahPertemuan"] = json!(20);
    let previous = parse_snapshot(old).unwrap();
    let reset = parse_snapshot(recap(2, "1")).unwrap();
    assert!(compare(&previous, &reset).is_empty());
}

#[test]
fn nim_resolution_requires_exactly_one_exact_match() {
    assert_eq!(
        resolve_student(
            json!({"Items":[{"NIM":"G6401260001","MahasiswaID":42}]}),
            "G6401260001"
        )
        .unwrap(),
        42
    );
    assert!(resolve_student(
        json!({"Items":[{"NIM":"OTHER","MahasiswaID":42}]}),
        "G6401260001"
    )
    .is_err());
    assert!(resolve_student(json!({"Items":[]}), "G6401260001").is_err());
    assert!(resolve_student(json!({"Items":[{"NIM":"G6401260001","MahasiswaID":42},{"NIM":"G6401260001","MahasiswaID":43}]}), "G6401260001").is_err());
}

#[test]
fn command_accepts_only_one_nim_or_off() {
    assert!(
        matches!(classify_message("#remind-absen g6401260001"), MessageType::Command(BotCommand::RemindAbsen(ReminderAction::Enable(nim))) if nim == "G6401260001")
    );
    assert!(matches!(
        classify_message("#remind-absen off"),
        MessageType::Command(BotCommand::RemindAbsen(ReminderAction::Disable))
    ));
    for input in [
        "#remind-absen",
        "#remind-absen abc",
        "#remind-absen G6401260001 extra",
        "#remind-absen off extra",
    ] {
        assert!(matches!(
            classify_message(input),
            MessageType::Command(BotCommand::MissingArgument(_))
        ));
    }
}
