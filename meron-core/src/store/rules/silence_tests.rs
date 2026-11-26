use super::*;
use crate::imap::MessageHeader;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::store::run_migrations(&conn).unwrap();
    conn
}

fn arrive_in(conn: &Connection, folder: &str, mail: &[(u32, &str)]) {
    let batch: Vec<MessageHeader> = mail
        .iter()
        .map(|(uid, from)| MessageHeader {
            uid: *uid,
            subject: "hello".into(),
            from_addr: (*from).into(),
            message_id: format!("<{uid}@example.com>"),
            ..Default::default()
        })
        .collect();
    crate::store::upsert_messages(conn, "a", folder, &batch).unwrap();
}

fn arrive(conn: &Connection, mail: &[(u32, &str)]) {
    arrive_in(conn, "INBOX", mail);
}

fn quiet_from(from: &str) -> Rule {
    Rule {
        id: from.into(),
        conditions: vec![RuleCondition::new("from", from)],
        silence: true,
        ..Default::default()
    }
}

fn quiet_sender(address: &str) -> Rule {
    Rule {
        id: address.into(),
        conditions: vec![RuleCondition::new("sender", address)],
        silence: true,
        ..Default::default()
    }
}

#[test]
fn a_silencing_rule_quiets_only_the_mail_it_matches() {
    let conn = db();
    set_rules(&conn, vec![quiet_from("news@shop.example")]).unwrap();
    arrive(
        &conn,
        &[(1, "news@shop.example"), (2, "friend@example.com")],
    );
    let quiet = silenced_arrivals(&conn, "a", "INBOX", &[1, 2]).unwrap();
    assert_eq!(quiet, BTreeSet::from([1]));
}

#[test]
fn a_sender_condition_takes_that_address_and_no_other_containing_it() {
    let conn = db();
    set_rules(&conn, vec![quiet_sender("News@Shop.example")]).unwrap();
    arrive(
        &conn,
        &[
            (1, "breakingnews@shop.example"),
            (2, "news@shop.example.org"),
            (3, "NEWS@shop.example"),
        ],
    );
    let quiet = silenced_arrivals(&conn, "a", "INBOX", &[1, 2, 3]).unwrap();
    assert_eq!(quiet, BTreeSet::from([3]));
    assert!(set_rules(&conn, vec![quiet_sender("a@x.example, b@x.example")]).is_err());
    assert!(set_rules(&conn, vec![quiet_sender("shop.example")]).is_err());
}

#[test]
fn disabled_and_other_account_rules_leave_notifications_alone() {
    let conn = db();
    set_rules(
        &conn,
        vec![
            Rule {
                disabled: true,
                ..quiet_from("news@shop.example")
            },
            Rule {
                id: "other".into(),
                accounts: vec!["b".into()],
                ..quiet_from("friend@example.com")
            },
        ],
    )
    .unwrap();
    arrive(
        &conn,
        &[(1, "news@shop.example"), (2, "friend@example.com")],
    );
    assert!(
        silenced_arrivals(&conn, "a", "INBOX", &[1, 2])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rules_never_silence_in_the_folders_they_never_run_on() {
    let conn = db();
    set_rules(&conn, vec![quiet_from("news@shop.example")]).unwrap();
    arrive_in(&conn, "Junk", &[(1, "news@shop.example")]);
    assert!(
        silenced_arrivals(&conn, "a", "Junk", &[1])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn only_a_rule_acting_right_away_on_received_mail_can_silence() {
    let conn = db();
    assert!(set_rules(&conn, vec![quiet_from("news@shop.example")]).is_ok());
    for rule in [
        Rule {
            delay_seconds: 60,
            ..quiet_from("news@shop.example")
        },
        Rule {
            trigger: RuleTrigger::Read,
            ..quiet_from("news@shop.example")
        },
        Rule {
            sent: true,
            ..quiet_from("news@shop.example")
        },
    ] {
        assert!(set_rules(&conn, vec![rule]).is_err());
    }
}

#[test]
fn rules_that_only_silence_are_kept_where_older_releases_dont_look() {
    let conn = db();
    let starring = Rule {
        id: "star".into(),
        star: true,
        ..Default::default()
    };
    let rules = vec![
        quiet_sender("news@shop.example"),
        starring.clone(),
        Rule {
            star: true,
            ..quiet_from("a@x.example")
        },
    ];
    set_rules(&conn, rules).unwrap();
    let stored = crate::store::settings_get(
        &conn,
        &[RULES_KEY.to_string(), SILENT_RULES_KEY.to_string()],
    )
    .unwrap();
    let ids = |key: &str| -> Vec<String> {
        stored[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["id"].as_str().unwrap().to_string())
            .collect()
    };
    // What an older release reads has an action it knows in every rule.
    assert_eq!(ids(RULES_KEY), ["star", "a@x.example"]);
    assert_eq!(ids(SILENT_RULES_KEY), ["news@shop.example"]);
    let read: Vec<String> = rules_of(&conn);
    assert_eq!(read, ["star", "a@x.example", "news@shop.example"]);
}

fn rules_of(conn: &Connection) -> Vec<String> {
    rules(conn)
        .unwrap()
        .into_iter()
        .map(|rule| rule.id)
        .collect()
}

#[test]
fn silencing_alone_leaves_the_activity_log_to_rules_that_act() {
    let conn = db();
    set_rules(&conn, vec![quiet_from("news@shop.example")]).unwrap();
    arrive(&conn, &[(1, "news@shop.example")]);
    apply_rules(&conn, "a", "INBOX", 7, &[1]).unwrap();
    assert!(rule_log(&conn).unwrap().is_empty());
}

#[test]
fn rules_from_before_silencing_still_notify() {
    let old: Rule = serde_json::from_str(r#"{"id":"old","star":true}"#).unwrap();
    assert!(!old.silence);
}
