//! The Ledger tab: the chart of accounts with balances, the postings of the
//! chosen account, the books' validation issues, and the two entry forms.
//!
//! Everything comes from the running server's `sapphire-ledger.*` methods
//! ([`LedgerClient`]), so the tab shows the workspace the server has
//! selected. Calls run on the tokio runtime and come back through a channel;
//! the view itself never blocks.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use sapphire_framework::gui::egui;
use sapphire_ledger_api::{
    self as api, Account, EmptyParams, PostingHit, QueryPostingsParams, ValidationIssue,
};

use crate::client::LedgerClient;
use crate::form::{self, AccountForm, PostingRow, Status, TagsForm, TxForm};

/// How often the tab re-reads the books on its own, to pick up what the CLI,
/// an agent or sync wrote.
const REFRESH_EVERY: Duration = Duration::from_secs(10);

/// What came back from the server.
enum Reply {
    /// One refresh: the accounts, every posting, and the validation issues.
    Books(Result<Books, String>),
    /// A form was sent; the text is the server's confirmation.
    Written(Sent, Result<String, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sent {
    Transaction,
    Account,
    Tags,
}

struct Books {
    accounts: Vec<Account>,
    postings: Vec<PostingHit>,
    issues: Vec<ValidationIssue>,
}

pub struct LedgerView {
    runtime: tokio::runtime::Handle,
    client: Arc<LedgerClient>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    replies: (mpsc::Sender<Reply>, mpsc::Receiver<Reply>),

    books: Option<Books>,
    /// Why the books could not be read, if the last refresh failed.
    load_error: Option<String>,
    loading: bool,
    last_refresh: Option<Instant>,
    /// The account whose postings are shown; `None` shows every posting.
    selected: Option<String>,
    /// The last write's confirmation, shown under the toolbar.
    notice: Option<String>,

    tx_form: Option<TxForm>,
    account_form: Option<AccountForm>,
    tags_form: Option<TagsForm>,
    /// A form is on its way to the server.
    sending: bool,
    /// Why the open form was not sent (or was refused).
    form_error: Option<String>,
}

impl LedgerView {
    pub fn new(
        runtime: tokio::runtime::Handle,
        client: Arc<LedgerClient>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> LedgerView {
        LedgerView {
            runtime,
            client,
            repaint,
            replies: mpsc::channel(),
            books: None,
            load_error: None,
            loading: false,
            last_refresh: None,
            selected: None,
            notice: None,
            tx_form: None,
            account_form: None,
            tags_form: None,
            sending: false,
            form_error: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.drain_replies();
        if !self.loading
            && self
                .last_refresh
                .is_none_or(|t| t.elapsed() >= REFRESH_EVERY)
        {
            self.refresh();
        }
        // Wake up for the next automatic refresh even if nothing else happens.
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        egui::Panel::top("ledger_toolbar").show(ui, |ui| self.toolbar(ui));
        if self.books.as_ref().is_some_and(|b| !b.issues.is_empty()) {
            egui::Panel::bottom("ledger_issues")
                .resizable(true)
                .default_size(120.0)
                .show(ui, |ui| self.issues(ui));
        }
        egui::Panel::left("ledger_accounts")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| self.accounts(ui));
        egui::CentralPanel::default().show(ui, |ui| self.postings(ui));

        self.tx_window(ui.ctx());
        self.account_window(ui.ctx());
        self.tags_window(ui.ctx());
    }

    // ── server calls ────────────────────────────────────────────────────────

    fn refresh(&mut self) {
        self.loading = true;
        self.last_refresh = Some(Instant::now());
        let client = Arc::clone(&self.client);
        self.spawn(async move {
            let books = async {
                let accounts = client.read(api::LIST_ACCOUNTS, EmptyParams {}).await?;
                let postings = client
                    .read(
                        api::QUERY_POSTINGS,
                        QueryPostingsParams {
                            account: None,
                            currency: None,
                            date_from: None,
                            date_to: None,
                        },
                    )
                    .await?;
                let issues = client.read(api::VALIDATE_WORKSPACE, EmptyParams {}).await?;
                Ok(Books {
                    accounts,
                    postings,
                    issues,
                })
            };
            Reply::Books(books.await)
        });
    }

    fn send_transaction(&mut self) {
        let Some(form) = &self.tx_form else { return };
        match form.to_params() {
            Ok(params) => {
                let client = Arc::clone(&self.client);
                self.start_sending();
                self.spawn(async move {
                    let result = client.call(api::ADD_TRANSACTION, params).await;
                    Reply::Written(Sent::Transaction, result)
                });
            }
            Err(e) => self.form_error = Some(e),
        }
    }

    fn send_account(&mut self) {
        let Some(form) = &self.account_form else {
            return;
        };
        match form.to_params() {
            Ok(params) => {
                let client = Arc::clone(&self.client);
                self.start_sending();
                self.spawn(async move {
                    let result = client.call(api::ADD_ACCOUNT, params).await;
                    Reply::Written(Sent::Account, result)
                });
            }
            Err(e) => self.form_error = Some(e),
        }
    }

    fn send_tags(&mut self) {
        let Some(form) = &self.tags_form else {
            return;
        };
        let params = form.to_params();
        let client = Arc::clone(&self.client);
        self.start_sending();
        self.spawn(async move {
            let result = client.call(api::UPDATE_ACCOUNT, params).await;
            Reply::Written(Sent::Tags, result)
        });
    }

    fn start_sending(&mut self) {
        self.sending = true;
        self.form_error = None;
    }

    fn spawn(&self, call: impl Future<Output = Reply> + Send + 'static) {
        let tx = self.replies.0.clone();
        let repaint = Arc::clone(&self.repaint);
        self.runtime.spawn(async move {
            // The receiver lives as long as the view; a send after the window
            // closed has no one to tell.
            let _ = tx.send(call.await);
            repaint();
        });
    }

    fn drain_replies(&mut self) {
        while let Ok(reply) = self.replies.1.try_recv() {
            match reply {
                Reply::Books(Ok(books)) => {
                    if let Some(id) = &self.selected
                        && !books.accounts.iter().any(|a| &a.id == id)
                    {
                        self.selected = None;
                    }
                    self.books = Some(books);
                    self.load_error = None;
                    self.loading = false;
                }
                Reply::Books(Err(e)) => {
                    // Keep showing the last books: a server restart should not
                    // blank the window.
                    self.load_error = Some(e);
                    self.loading = false;
                }
                Reply::Written(sent, Ok(text)) => {
                    self.sending = false;
                    match sent {
                        Sent::Transaction => self.tx_form = None,
                        Sent::Account => self.account_form = None,
                        Sent::Tags => self.tags_form = None,
                    }
                    self.notice = Some(first_line(&text));
                    self.refresh();
                }
                Reply::Written(_, Err(e)) => {
                    self.sending = false;
                    // The form stays open with what was typed.
                    self.form_error = Some(e);
                }
            }
        }
    }

    // ── panels ──────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_write = self.load_error.is_none() && self.books.is_some();
            if ui
                .add_enabled(can_write, egui::Button::new("New transaction"))
                .clicked()
            {
                let currency = self.default_currency();
                self.tx_form = Some(TxForm::new(today(), &currency));
                self.form_error = None;
            }
            if ui
                .add_enabled(can_write, egui::Button::new("New account"))
                .clicked()
            {
                self.account_form = Some(AccountForm::new(today()));
                self.form_error = None;
            }
            if ui
                .add_enabled(!self.loading, egui::Button::new("Refresh"))
                .clicked()
            {
                self.refresh();
            }
            if self.loading {
                ui.spinner();
            }
        });
        if let Some(e) = &self.load_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some(text) = &self.notice {
            ui.label(text);
        }
    }

    fn accounts(&mut self, ui: &mut egui::Ui) {
        let Some(books) = &self.books else {
            ui.label("Loading…");
            return;
        };
        let balances = balances(&books.accounts, &books.postings);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.selectable_value(&mut self.selected, None, "All postings");
            if books.accounts.is_empty() {
                ui.add_space(8.0);
                ui.label("No accounts yet. Every posting needs one: start with New account.");
            }
            for ty in form::ACCOUNT_TYPES {
                let mut of_type: Vec<&Account> = books
                    .accounts
                    .iter()
                    .filter(|a| a.account_type == ty)
                    .collect();
                if of_type.is_empty() {
                    continue;
                }
                of_type.sort_by(|a, b| a.name.cmp(&b.name));
                ui.add_space(6.0);
                ui.strong(form::type_name(ty));
                for account in of_type {
                    let selected = self.selected.as_deref() == Some(account.id.as_str());
                    let total = balances
                        .get(&account.id)
                        .map(format_amounts)
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        if ui.selectable_label(selected, &account.name).clicked() {
                            self.selected = Some(account.id.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.monospace(total);
                        });
                    });
                }
            }
        });
    }

    fn postings(&mut self, ui: &mut egui::Ui) {
        if let Some(form) = self.postings_panel(ui) {
            self.tags_form = Some(form);
            self.form_error = None;
        }
    }

    /// The postings of the selected account (or all), and the tags form if
    /// "Edit tags" was clicked.
    fn postings_panel(&self, ui: &mut egui::Ui) -> Option<TagsForm> {
        let mut edit_tags = None;
        let Some(books) = &self.books else {
            return None;
        };
        let account = self
            .selected
            .as_ref()
            .and_then(|id| books.accounts.iter().find(|a| &a.id == id));
        ui.heading(account.map_or("All postings", |a| a.name.as_str()));
        if let Some(a) = account.and_then(|a| a.description.as_deref()) {
            ui.label(a);
        }
        if let Some(a) = account {
            ui.horizontal(|ui| {
                ui.label(if a.default_tags.is_empty() {
                    "No default tags".to_owned()
                } else {
                    format!("Default tags: {}", a.default_tags.join(", "))
                });
                if ui.small_button("Edit tags").clicked() {
                    edit_tags = Some(TagsForm::new(a));
                }
            });
        }
        ui.separator();

        let hits: Vec<&PostingHit> = books
            .postings
            .iter()
            .filter(|p| account.is_none_or(|a| posts_to(p, a)))
            .collect();
        if hits.is_empty() {
            ui.label("No postings.");
            return edit_tags;
        }
        // The running balance only means something within one account.
        let running = account.map(|_| running_balances(&hits));
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("ledger_postings")
                .striped(true)
                .num_columns(5)
                .show(ui, |ui| {
                    ui.strong("Date");
                    ui.strong("Narration");
                    ui.strong(if account.is_some() {
                        "Amount"
                    } else {
                        "Account"
                    });
                    ui.strong(if account.is_some() {
                        "Balance"
                    } else {
                        "Amount"
                    });
                    ui.strong("Id");
                    ui.end_row();
                    for (i, hit) in hits.iter().enumerate() {
                        ui.label(hit.date.to_string());
                        ui.label(&hit.narration);
                        match &running {
                            Some(running) => {
                                ui.monospace(format!("{} {}", hit.amount, hit.currency));
                                ui.monospace(format!("{} {}", running[i], hit.currency));
                            }
                            None => {
                                ui.label(account_label(hit, &books.accounts));
                                ui.monospace(format!("{} {}", hit.amount, hit.currency));
                            }
                        }
                        ui.weak(&hit.transaction_id);
                        ui.end_row();
                    }
                });
        });
        edit_tags
    }

    fn issues(&self, ui: &mut egui::Ui) {
        let Some(books) = &self.books else { return };
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{} validation issue(s)", books.issues.len()),
        );
        egui::ScrollArea::vertical().show(ui, |ui| {
            for issue in &books.issues {
                ui.label(format!("• {}", issue.message));
            }
        });
    }

    // ── forms ───────────────────────────────────────────────────────────────

    fn tx_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.tx_form else {
            return;
        };
        let accounts: Vec<Account> = self
            .books
            .as_ref()
            .map(|b| {
                let mut accounts = b.accounts.clone();
                accounts.sort_by(|a, b| a.name.cmp(&b.name));
                accounts
            })
            .unwrap_or_default();
        let mut open = true;
        let mut submit = false;
        egui::Window::new("New transaction")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                egui::Grid::new("tx_fields").num_columns(2).show(ui, |ui| {
                    ui.label("Date");
                    ui.add(egui::TextEdit::singleline(&mut form.date).hint_text("YYYY-MM-DD"));
                    ui.end_row();
                    ui.label("Narration");
                    ui.add(egui::TextEdit::singleline(&mut form.narration).desired_width(360.0));
                    ui.end_row();
                    ui.label("Payee");
                    ui.text_edit_singleline(&mut form.payee);
                    ui.end_row();
                    ui.label("Tags");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.tags).hint_text("comma, separated"),
                    );
                    ui.end_row();
                    ui.label("Status");
                    egui::ComboBox::from_id_salt("tx_status")
                        .selected_text(form.status.label())
                        .show_ui(ui, |ui| {
                            for s in Status::ALL {
                                ui.selectable_value(&mut form.status, s, s.label());
                            }
                        });
                    ui.end_row();
                    ui.label("Comment");
                    ui.add(
                        egui::TextEdit::multiline(&mut form.comment)
                            .desired_rows(3)
                            .desired_width(360.0)
                            .hint_text("Markdown"),
                    );
                    ui.end_row();
                });

                ui.separator();
                ui.strong("Postings");
                let mut remove = None;
                // Default tags of the accounts chosen this frame.
                let mut picked: Vec<Vec<String>> = vec![];
                egui::Grid::new("tx_postings")
                    .num_columns(5)
                    .show(ui, |ui| {
                        for (i, row) in form.postings.iter_mut().enumerate() {
                            if let Some(a) = posting_row(ui, i, row, &accounts) {
                                picked.push(a.default_tags.clone());
                            }
                            if ui
                                .add_enabled(i >= 2, egui::Button::new("✕"))
                                .on_hover_text("Remove this posting")
                                .clicked()
                            {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                for tags in picked {
                    form.add_tags(&tags);
                }
                if let Some(i) = remove {
                    form.postings.remove(i);
                }
                ui.horizontal(|ui| {
                    if ui.button("Add posting").clicked() {
                        let currency = form
                            .postings
                            .last()
                            .map(|r| r.currency.clone())
                            .unwrap_or_default();
                        form.postings.push(PostingRow {
                            currency,
                            ..PostingRow::default()
                        });
                    }
                    if ui
                        .button("Balance last")
                        .on_hover_text("Fill the last amount so its currency sums to zero")
                        .clicked()
                        && let Some(amount) = form.balancing_amount()
                        && let Some(last) = form.postings.last_mut()
                    {
                        last.amount = amount.to_string();
                    }
                });
                match form.imbalance() {
                    Ok(off) if off.is_empty() => {
                        ui.label("Balanced.");
                    }
                    Ok(off) => {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("Off by {}", format_amounts(&off)),
                        );
                    }
                    Err(e) => {
                        ui.colored_label(ui.visuals().warn_fg_color, e);
                    }
                }

                ui.separator();
                if let Some(e) = &self.form_error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    submit = ui
                        .add_enabled(!self.sending, egui::Button::new("Record"))
                        .clicked();
                    if self.sending {
                        ui.spinner();
                    }
                });
            });
        if !open {
            self.tx_form = None;
            self.form_error = None;
        } else if submit {
            self.send_transaction();
        }
    }

    fn account_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.account_form else {
            return;
        };
        let mut open = true;
        let mut submit = false;
        egui::Window::new("New account")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("account_fields")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.name)
                                .hint_text("Expenses:Food")
                                .desired_width(280.0),
                        );
                        ui.end_row();
                        ui.label("Type");
                        egui::ComboBox::from_id_salt("account_type")
                            .selected_text(form::type_name(form.account_type))
                            .show_ui(ui, |ui| {
                                for ty in form::ACCOUNT_TYPES {
                                    ui.selectable_value(
                                        &mut form.account_type,
                                        ty,
                                        form::type_name(ty),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Currencies");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.currencies)
                                .hint_text("empty allows any; JPY, USD"),
                        );
                        ui.end_row();
                        ui.label("Default tags");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.default_tags).hint_text("living"),
                        );
                        ui.end_row();
                        ui.label("Opened");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.opened_at).hint_text("YYYY-MM-DD"),
                        );
                        ui.end_row();
                        ui.label("Description");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.description)
                                .desired_rows(3)
                                .hint_text("Markdown"),
                        );
                        ui.end_row();
                    });
                ui.separator();
                if let Some(e) = &self.form_error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    submit = ui
                        .add_enabled(!self.sending, egui::Button::new("Create"))
                        .clicked();
                    if self.sending {
                        ui.spinner();
                    }
                });
            });
        if !open {
            self.account_form = None;
            self.form_error = None;
        } else if submit {
            self.send_account();
        }
    }

    fn tags_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.tags_form else {
            return;
        };
        let mut open = true;
        let mut submit = false;
        egui::Window::new("Default tags")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(&form.account_name);
                ui.add(egui::TextEdit::singleline(&mut form.tags).hint_text("living, food"));
                ui.weak("Copied into each transaction recorded against this account from now on.");
                ui.separator();
                if let Some(e) = &self.form_error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    submit = ui
                        .add_enabled(!self.sending, egui::Button::new("Save"))
                        .clicked();
                    if self.sending {
                        ui.spinner();
                    }
                });
            });
        if !open {
            self.tags_form = None;
            self.form_error = None;
        } else if submit {
            self.send_tags();
        }
    }

    /// The currency a new transaction starts in: the selected account's
    /// first, else the one most postings use, else JPY.
    fn default_currency(&self) -> String {
        let Some(books) = &self.books else {
            return "JPY".to_owned();
        };
        if let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| books.accounts.iter().find(|a| &a.id == id))
            .and_then(|a| a.currencies.first())
        {
            return c.clone();
        }
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for p in &books.postings {
            *counts.entry(p.currency.as_str()).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map_or_else(|| "JPY".to_owned(), |(c, _)| c.to_owned())
    }
}

/// One posting's fields; returns the account newly chosen this frame.
fn posting_row<'a>(
    ui: &mut egui::Ui,
    i: usize,
    row: &mut PostingRow,
    accounts: &'a [Account],
) -> Option<&'a Account> {
    let mut chosen = None;
    egui::ComboBox::from_id_salt(("posting_account", i))
        .width(220.0)
        .selected_text(if row.account.is_empty() {
            "Account…"
        } else {
            row.account.as_str()
        })
        .show_ui(ui, |ui| {
            for account in accounts {
                if ui
                    .selectable_value(&mut row.account, account.name.clone(), &account.name)
                    .changed()
                {
                    chosen = Some(account);
                }
            }
        });
    ui.add(
        egui::TextEdit::singleline(&mut row.amount)
            .hint_text("-1200")
            .desired_width(100.0),
    );
    ui.add(
        egui::TextEdit::singleline(&mut row.currency)
            .hint_text("JPY")
            .desired_width(50.0),
    );
    ui.add(
        egui::TextEdit::singleline(&mut row.memo)
            .hint_text("memo")
            .desired_width(140.0),
    );
    chosen
}

// ── pure helpers ────────────────────────────────────────────────────────────

/// Does `hit` post to `account`? By id when the posting has one; a posting
/// carrying only a name (a hand-written record) matches by name.
fn posts_to(hit: &PostingHit, account: &Account) -> bool {
    match &hit.account_id {
        Some(id) => *id == account.id,
        None => hit.account_name.as_deref() == Some(account.name.as_str()),
    }
}

/// Each account's balance per currency, keyed by account id.
fn balances(
    accounts: &[Account],
    postings: &[PostingHit],
) -> BTreeMap<String, BTreeMap<String, Decimal>> {
    let mut out: BTreeMap<String, BTreeMap<String, Decimal>> = BTreeMap::new();
    for hit in postings {
        let Some(account) = accounts.iter().find(|a| posts_to(hit, a)) else {
            continue;
        };
        let Some(amount) = form::parse_amount(&hit.amount) else {
            continue;
        };
        *out.entry(account.id.clone())
            .or_default()
            .entry(hit.currency.clone())
            .or_default() += amount;
    }
    out
}

/// The balance after each of `hits`, in that posting's currency.
fn running_balances(hits: &[&PostingHit]) -> Vec<Decimal> {
    let mut sums: BTreeMap<&str, Decimal> = BTreeMap::new();
    hits.iter()
        .map(|hit| {
            let sum = sums.entry(hit.currency.as_str()).or_default();
            *sum += form::parse_amount(&hit.amount).unwrap_or_default();
            *sum
        })
        .collect()
}

/// `1200 JPY, 5 USD`; currencies whose sum is zero are left out.
fn format_amounts(amounts: &BTreeMap<String, Decimal>) -> String {
    amounts
        .iter()
        .filter(|(_, a)| !a.is_zero())
        .map(|(c, a)| format!("{a} {c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The name to show for a posting's account: the account's current name when
/// it is known, else what the posting itself carries.
fn account_label(hit: &PostingHit, accounts: &[Account]) -> String {
    accounts
        .iter()
        .find(|a| posts_to(hit, a))
        .map(|a| a.name.clone())
        .or_else(|| hit.account_name.clone())
        .or_else(|| hit.account_id.clone())
        .unwrap_or_default()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

fn today() -> String {
    chrono::Local::now().date_naive().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sapphire_ledger_api::AccountType;

    fn account(id: &str, name: &str) -> Account {
        Account {
            id: id.into(),
            name: name.into(),
            account_type: AccountType::Asset,
            currencies: vec![],
            opened_at: "2026-01-01".parse().unwrap(),
            closed_at: None,
            default_tags: vec![],
            description: None,
        }
    }

    fn hit(id: Option<&str>, name: Option<&str>, amount: &str, currency: &str) -> PostingHit {
        PostingHit {
            transaction_id: "tx00001".into(),
            date: "2026-05-21".parse().unwrap(),
            narration: "n".into(),
            account_id: id.map(Into::into),
            account_name: name.map(Into::into),
            amount: amount.into(),
            currency: currency.into(),
        }
    }

    #[test]
    fn the_id_decides_and_a_stale_name_does_not_matter() {
        let cash = account("acc0001", "Assets:Cash");
        assert!(posts_to(
            &hit(Some("acc0001"), Some("Assets:Old"), "1", "JPY"),
            &cash
        ));
        assert!(!posts_to(
            &hit(Some("acc0002"), Some("Assets:Cash"), "1", "JPY"),
            &cash
        ));
        assert!(posts_to(&hit(None, Some("Assets:Cash"), "1", "JPY"), &cash));
    }

    #[test]
    fn balances_sum_per_account_and_currency() {
        let accounts = [account("a", "Assets:Cash"), account("b", "Expenses:Food")];
        let postings = [
            hit(Some("a"), None, "-1200", "JPY"),
            hit(Some("b"), None, "1200", "JPY"),
            hit(Some("a"), None, "100", "JPY"),
            hit(Some("a"), None, "5", "USD"),
            hit(Some("zzz"), None, "7", "JPY"),
        ];
        let b = balances(&accounts, &postings);
        assert_eq!(b["a"]["JPY"], Decimal::from(-1100));
        assert_eq!(b["a"]["USD"], Decimal::from(5));
        assert_eq!(b["b"]["JPY"], Decimal::from(1200));
        assert_eq!(b.len(), 2, "a posting to an unknown account is nobody's");
        assert_eq!(format_amounts(&b["a"]), "-1100 JPY, 5 USD");
    }

    #[test]
    fn the_running_balance_keeps_currencies_apart() {
        let postings = [
            hit(Some("a"), None, "100", "JPY"),
            hit(Some("a"), None, "5", "USD"),
            hit(Some("a"), None, "-30", "JPY"),
        ];
        let refs: Vec<&PostingHit> = postings.iter().collect();
        assert_eq!(
            running_balances(&refs),
            [Decimal::from(100), Decimal::from(5), Decimal::from(70)]
        );
    }
}
