use super::*;
use crate::imap::MessageHeader;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::store::run_migrations(&conn).unwrap();
    conn
}

fn arrive(conn: &Connection, mail: &[(u32, &str)]) {
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
    crate::store::upsert_messages(conn, "a", "INBOX", &batch).unwrap();
}

fn quiet_from(from: &str) -> Rule {
    Rule {
        id: from.into(),
        conditions: vec![RuleCondition::new("from", from)],
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
fn rules_from_before_silencing_still_notify() {
    let old: Rule = serde_json::from_str(r#"{"id":"old","star":true}"#).unwrap();
    assert!(!old.silence);
}
