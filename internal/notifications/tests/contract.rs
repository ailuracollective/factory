use notifications::*;

fn sample() -> Notification {
    Notification::new(
        NotificationKind::AgentCompleted,
        "Agent finished",
        "PR #42 opened",
        Sender {
            id: "club-1".into(),
            name: "Club".into(),
            avatar_url: None,
        },
        Recipient {
            bot_id: "bot-1".into(),
            channel: "general".into(),
        },
    )
}

#[test]
fn validates() {
    let n = sample();
    assert!(n.validate().is_ok());
}

#[test]
fn rejects_empty_title() {
    let mut n = sample();
    n.title = "".into();
    assert!(n.validate().is_err());
}

#[test]
fn round_trips_json() {
    let n = sample().with_data("issue", serde_json::json!(7));
    let s = serde_json::to_string(&n).unwrap();
    let back: Notification = serde_json::from_str(&s).unwrap();
    assert_eq!(back, n);
}

#[test]
fn fcm_round_trip() {
    let n = sample();
    let msg = n.to_fcm("device-token");
    assert_eq!(msg.token, "device-token");
    assert_eq!(msg.notification.title, n.title);
    let back = from_fcm_data(&msg.data).unwrap();
    assert_eq!(back, n);
}

#[test]
fn kind_serializes_snake_case() {
    let s = serde_json::to_string(&NotificationKind::AgentCompleted).unwrap();
    assert_eq!(s, "\"agent_completed\"");
}
