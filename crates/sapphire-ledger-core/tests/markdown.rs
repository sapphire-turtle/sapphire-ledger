use sapphire_ledger_core::{Account, Transaction, from_markdown, to_markdown};

const FRONT: &str = "id: acct001\nname: Assets:Cash\ntype: Asset\nopened_at: 2026-05-21\n";
/// [`FRONT`] as the ledger writes it: a date is quoted so that no YAML reader
/// takes it for a timestamp.
const WRITTEN: &str = "id: acct001\nname: Assets:Cash\ntype: Asset\nopened_at: \"2026-05-21\"\n";

#[test]
fn the_description_is_the_body_and_stays_out_of_the_front_matter() {
    let account: Account =
        from_markdown(&format!("---\n{FRONT}---\n\n# Wallet\n\nCoins too.\n")).unwrap();
    assert_eq!(
        account.description.as_deref(),
        Some("# Wallet\n\nCoins too.")
    );

    let text = to_markdown(&account).unwrap();
    assert_eq!(
        text,
        format!("---\n{WRITTEN}---\n\n# Wallet\n\nCoins too.\n")
    );
}

#[test]
fn no_body_means_no_description_and_no_blank_tail() {
    let account: Account = from_markdown(&format!("---\n{FRONT}---\n\n  \n")).unwrap();
    assert_eq!(account.description, None);
    assert_eq!(
        to_markdown(&account).unwrap(),
        format!("---\n{WRITTEN}---\n")
    );
}

/// Someone writing the file by hand should not have to know which field
/// lives where.
#[test]
fn a_description_in_the_front_matter_is_accepted_while_the_body_is_empty() {
    let account: Account =
        from_markdown(&format!("---\n{FRONT}description: the wallet\n---\n")).unwrap();
    assert_eq!(account.description.as_deref(), Some("the wallet"));
}

#[test]
fn a_description_in_both_places_is_refused() {
    let err = from_markdown::<Account>(&format!(
        "---\n{FRONT}description: the wallet\n---\n\nAlso the wallet.\n"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("description"), "got: {err}");
}

#[test]
fn a_transaction_round_trips_with_strings_yaml_would_misread() {
    let tx: Transaction = toml::from_str(
        r#"
id = "1234567"
date = "2026-05-21"
narration = "- yes: #1 'quote'"
payee = "no"
tags = ["yes", "null", "true", "1.0", "~"]
created_at = "2026-05-21T18:30:00+09:00"
updated_at = "2026-05-21T18:30:00+09:00"
comment = "line one\n\nline two"

[[postings]]
account_name = "Expenses:Food"
amount = "1200.50"
currency = "NO"
price = { value = "150", currency = "JPY" }
memo = "two\nlines"

[[postings]]
account_name = "Assets: Cash"
amount = "-180075"
currency = "JPY"
"#,
    )
    .unwrap();
    let text = to_markdown(&tx).unwrap();
    assert!(
        text.ends_with("---\n\nline one\n\nline two\n"),
        "got:\n{text}"
    );
    assert_eq!(from_markdown::<Transaction>(&text).unwrap(), tx);
}

/// Hand-written YAML leaves amounts unquoted; they must keep their digits.
#[test]
fn an_unquoted_amount_keeps_its_precision() {
    let tx: Transaction = from_markdown(
        "---\nid: t\ndate: 2026-05-21\nnarration: x\n\
         created_at: 2026-05-21T18:30:00+09:00\nupdated_at: 2026-05-21T18:30:00+09:00\n\
         postings:\n- account_name: A\n  amount: 1200.50\n  currency: JPY\n\
         - account_name: B\n  amount: -1200.5\n  currency: JPY\n---\n",
    )
    .unwrap();
    assert_eq!(tx.postings[0].amount.to_string(), "1200.50");
    tx.validate().expect("balances");
}
