//! The Ledger tab: the chart of accounts with balances, the postings of the
//! chosen account, the books' validation issues, and in the side pane the
//! picked transaction or account and the entry forms.
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
use sapphire_framework::gui::{Side, egui};
use sapphire_ledger_api::{
    self as api, Account, EmptyParams, GetTransactionParams, PostingHit, QueryPostingsParams,
    Transaction, TransactionStatus, ValidationIssue,
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
    /// The picked transaction, by id.
    Transaction(String, Result<Transaction, String>),
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

/// What the side pane shows when no form is open, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    Transaction(String),
    Account(String),
}

/// The form open in the side pane; one at a time.
enum Editor {
    Transaction(TxForm),
    Account(AccountForm),
    Tags(TagsForm),
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

    /// The transaction or account shown in the side pane.
    picked: Option<Pick>,
    /// The picked transaction as `get_transaction` answered; `None` while
    /// it is on its way.
    transaction: Option<(String, Result<Transaction, String>)>,

    /// The form in the side pane, shown in place of the picked item.
    editor: Option<Editor>,
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
            picked: None,
            transaction: None,
            editor: None,
            sending: false,
            form_error: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        if self.drain_replies() {
            side.close();
        }
        if !self.loading
            && self
                .last_refresh
                .is_none_or(|t| t.elapsed() >= REFRESH_EVERY)
        {
            self.refresh();
        }
        // Wake up for the next automatic refresh even if nothing else happens.
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        egui::Panel::top("ledger_toolbar").show(ui, |ui| self.toolbar(ui, side));
        if self.books.as_ref().is_some_and(|b| !b.issues.is_empty()) {
            egui::Panel::bottom("ledger_issues")
                .resizable(true)
                .default_size(120.0)
                .show(ui, |ui| self.issues(ui));
        }
        egui::Panel::left("ledger_accounts")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| self.accounts(ui, side));
        egui::CentralPanel::default().show(ui, |ui| self.postings(ui, side));
    }

    /// The side pane: the open form, else the picked transaction or account.
    pub fn side_ui(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.editor.is_some() {
                self.form_ui(ui, side);
                return;
            }
            match self.picked.clone() {
                Some(Pick::Transaction(id)) => self.transaction_detail(ui, &id),
                Some(Pick::Account(id)) => self.account_detail(ui, &id),
                None => {
                    ui.weak("Nothing selected.");
                }
            }
        });
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

    fn fetch_transaction(&mut self, id: String) {
        self.transaction = None;
        let client = Arc::clone(&self.client);
        self.spawn(async move {
            let result = client
                .read(
                    api::GET_TRANSACTION,
                    GetTransactionParams { id: id.clone() },
                )
                .await;
            Reply::Transaction(id, result)
        });
    }

    fn send_transaction(&mut self) {
        let Some(Editor::Transaction(form)) = &self.editor else {
            return;
        };
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
        let Some(Editor::Account(form)) = &self.editor else {
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
        let Some(Editor::Tags(form)) = &self.editor else {
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

    /// Takes in what came back; true when a new transaction or account was
    /// recorded, and the side pane is done with its form.
    fn drain_replies(&mut self) -> bool {
        let mut done = false;
        while let Ok(reply) = self.replies.1.try_recv() {
            match reply {
                Reply::Books(Ok(books)) => {
                    if let Some(id) = &self.selected
                        && !books.accounts.iter().any(|a| &a.id == id)
                    {
                        self.selected = None;
                    }
                    if self
                        .picked
                        .as_ref()
                        .is_some_and(|p| !still_there(p, &books))
                    {
                        self.picked = None;
                        self.transaction = None;
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
                Reply::Transaction(id, result) => {
                    // A reply for a transaction no longer picked is dropped.
                    if self.picked == Some(Pick::Transaction(id.clone())) {
                        self.transaction = Some((id, result));
                    }
                }
                Reply::Written(sent, Ok(text)) => {
                    self.sending = false;
                    self.editor = None;
                    // New tags go back to their account; a new record has
                    // nothing more to show.
                    done |= sent != Sent::Tags;
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
        done
    }

    // ── picking and forms ───────────────────────────────────────────────────

    fn pick_transaction(&mut self, id: String, side: &mut Side) {
        if self.picked.as_ref() != Some(&Pick::Transaction(id.clone())) {
            self.picked = Some(Pick::Transaction(id.clone()));
            self.fetch_transaction(id);
        }
        side.open();
    }

    fn pick_account(&mut self, id: String, side: &mut Side) {
        self.selected = Some(id.clone());
        self.picked = Some(Pick::Account(id));
        self.transaction = None;
        side.open();
    }

    fn open_editor(&mut self, editor: Editor, side: &mut Side) {
        self.editor = Some(editor);
        self.form_error = None;
        side.open();
    }

    /// Drops the open form: back to the picked item, or out of the pane if
    /// there is none.
    fn cancel_editor(&mut self, side: &mut Side) {
        self.editor = None;
        self.form_error = None;
        if self.picked.is_none() {
            side.close();
        }
    }

    // ── panels ──────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        ui.horizontal(|ui| {
            let can_write = self.load_error.is_none() && self.books.is_some();
            if ui
                .add_enabled(can_write, egui::Button::new("New transaction"))
                .clicked()
            {
                let currency = self.default_currency();
                self.open_editor(Editor::Transaction(TxForm::new(today(), &currency)), side);
            }
            if ui
                .add_enabled(can_write, egui::Button::new("New account"))
                .clicked()
            {
                self.open_editor(Editor::Account(AccountForm::new(today())), side);
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

    fn accounts(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        let Some(books) = &self.books else {
            ui.label("Loading…");
            return;
        };
        let balances = balances(&books.accounts, &books.postings);
        let mut all = false;
        let mut clicked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui
                .selectable_label(self.selected.is_none(), "All postings")
                .clicked()
            {
                all = true;
            }
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
                            clicked = Some(account.id.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.monospace(total);
                        });
                    });
                }
            }
        });
        if all {
            self.selected = None;
            if matches!(self.picked, Some(Pick::Account(_))) {
                self.picked = None;
            }
        }
        if let Some(id) = clicked {
            self.pick_account(id, side);
        }
    }

    fn postings(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        if let Some(id) = self.postings_panel(ui) {
            self.pick_transaction(id, side);
        }
    }

    /// The postings of the selected account (or all), and the transaction
    /// whose row was clicked.
    fn postings_panel(&self, ui: &mut egui::Ui) -> Option<String> {
        let mut clicked = None;
        let books = self.books.as_ref()?;
        let account = self
            .selected
            .as_ref()
            .and_then(|id| books.accounts.iter().find(|a| &a.id == id));
        ui.heading(account.map_or("All postings", |a| a.name.as_str()));
        if let Some(a) = account.and_then(|a| a.description.as_deref()) {
            ui.label(a);
        }
        ui.separator();

        let hits: Vec<&PostingHit> = books
            .postings
            .iter()
            .filter(|p| account.is_none_or(|a| posts_to(p, a)))
            .collect();
        if hits.is_empty() {
            ui.label("No postings.");
            return None;
        }
        let picked = match &self.picked {
            Some(Pick::Transaction(id)) => Some(id.as_str()),
            _ => None,
        };
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
                        let on = picked == Some(hit.transaction_id.as_str());
                        if ui.selectable_label(on, &hit.narration).clicked() {
                            clicked = Some(hit.transaction_id.clone());
                        }
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
        clicked
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

    // ── side pane ───────────────────────────────────────────────────────────

    fn transaction_detail(&self, ui: &mut egui::Ui, id: &str) {
        let tx = match &self.transaction {
            Some((of, Ok(tx))) if of == id => tx,
            Some((of, Err(e))) if of == id => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
            _ => {
                ui.spinner();
                return;
            }
        };
        let accounts = self.books.as_ref().map_or(&[][..], |b| &b.accounts[..]);
        ui.heading(&tx.narration);
        egui::Grid::new("tx_detail").num_columns(2).show(ui, |ui| {
            ui.label("Date");
            ui.label(tx.date.to_string());
            ui.end_row();
            if let Some(payee) = &tx.payee {
                ui.label("Payee");
                ui.label(payee);
                ui.end_row();
            }
            if !tx.tags.is_empty() {
                ui.label("Tags");
                ui.label(tx.tags.join(", "));
                ui.end_row();
            }
            if let Some(status) = tx.status {
                ui.label("Status");
                ui.label(match status {
                    TransactionStatus::Cleared => "cleared",
                    TransactionStatus::Pending => "pending",
                });
                ui.end_row();
            }
            ui.label("Id");
            ui.weak(&tx.id);
            ui.end_row();
        });

        ui.separator();
        ui.strong("Postings");
        egui::Grid::new("tx_detail_postings")
            .striped(true)
            .num_columns(2)
            .show(ui, |ui| {
                for p in &tx.postings {
                    let name = accounts
                        .iter()
                        .find(|a| p.account_id.as_deref() == Some(a.id.as_str()))
                        .map(|a| a.name.clone())
                        .or_else(|| p.account_name.clone())
                        .or_else(|| p.account_id.clone())
                        .unwrap_or_default();
                    ui.vertical(|ui| {
                        ui.label(name);
                        if let Some(memo) = &p.memo {
                            ui.weak(memo);
                        }
                    });
                    let mut amount = format!("{} {}", p.amount, p.currency);
                    if let Some(price) = &p.price {
                        amount.push_str(&format!(" @ {} {}", price.value, price.currency));
                    }
                    ui.monospace(amount);
                    ui.end_row();
                }
            });

        if let Some(comment) = &tx.comment {
            ui.separator();
            ui.label(comment);
        }
    }

    fn account_detail(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(books) = &self.books else {
            ui.spinner();
            return;
        };
        let Some(account) = books.accounts.iter().find(|a| a.id == id) else {
            ui.weak("Nothing selected.");
            return;
        };
        let balance = balances(&books.accounts, &books.postings)
            .get(&account.id)
            .map(format_amounts)
            .unwrap_or_default();
        let mut edit_tags = None;
        ui.heading(&account.name);
        egui::Grid::new("account_detail")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Type");
                ui.label(form::type_name(account.account_type));
                ui.end_row();
                ui.label("Balance");
                ui.monospace(if balance.is_empty() { "0" } else { &balance });
                ui.end_row();
                ui.label("Currencies");
                ui.label(if account.currencies.is_empty() {
                    "any".to_owned()
                } else {
                    account.currencies.join(", ")
                });
                ui.end_row();
                ui.label("Opened");
                ui.label(account.opened_at.to_string());
                ui.end_row();
                if let Some(closed) = account.closed_at {
                    ui.label("Closed");
                    ui.label(closed.to_string());
                    ui.end_row();
                }
                ui.label("Default tags");
                ui.horizontal(|ui| {
                    ui.label(if account.default_tags.is_empty() {
                        "none".to_owned()
                    } else {
                        account.default_tags.join(", ")
                    });
                    if ui.small_button("Edit tags").clicked() {
                        edit_tags = Some(TagsForm::new(account));
                    }
                });
                ui.end_row();
                ui.label("Id");
                ui.weak(&account.id);
                ui.end_row();
            });
        if let Some(description) = &account.description {
            ui.separator();
            ui.label(description);
        }
        if let Some(form) = edit_tags {
            self.editor = Some(Editor::Tags(form));
            self.form_error = None;
        }
    }

    /// The open form, with its Record (or Save) and Cancel.
    fn form_ui(&mut self, ui: &mut egui::Ui, side: &mut Side) {
        let accounts: Vec<Account> = self
            .books
            .as_ref()
            .map(|b| {
                let mut accounts = b.accounts.clone();
                accounts.sort_by(|a, b| a.name.cmp(&b.name));
                accounts
            })
            .unwrap_or_default();
        let (submit_label, sent) = match &mut self.editor {
            Some(Editor::Transaction(form)) => {
                ui.heading("New transaction");
                tx_fields(ui, form, &accounts);
                ("Record", Sent::Transaction)
            }
            Some(Editor::Account(form)) => {
                ui.heading("New account");
                account_fields(ui, form);
                ("Create", Sent::Account)
            }
            Some(Editor::Tags(form)) => {
                ui.heading("Default tags");
                ui.label(&form.account_name);
                ui.add(egui::TextEdit::singleline(&mut form.tags).hint_text("living, food"));
                ui.weak("Copied into each transaction recorded against this account from now on.");
                ("Save", Sent::Tags)
            }
            None => return,
        };

        ui.separator();
        if let Some(e) = &self.form_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        let mut submit = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            submit = ui
                .add_enabled(!self.sending, egui::Button::new(submit_label))
                .clicked();
            cancel = ui
                .add_enabled(!self.sending, egui::Button::new("Cancel"))
                .clicked();
            if self.sending {
                ui.spinner();
            }
        });
        if cancel {
            self.cancel_editor(side);
        } else if submit {
            match sent {
                Sent::Transaction => self.send_transaction(),
                Sent::Account => self.send_account(),
                Sent::Tags => self.send_tags(),
            }
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

/// A new transaction's fields and postings.
fn tx_fields(ui: &mut egui::Ui, form: &mut TxForm, accounts: &[Account]) {
    egui::Grid::new("tx_fields").num_columns(2).show(ui, |ui| {
        ui.label("Date");
        ui.add(egui::TextEdit::singleline(&mut form.date).hint_text("YYYY-MM-DD"));
        ui.end_row();
        ui.label("Narration");
        ui.text_edit_singleline(&mut form.narration);
        ui.end_row();
        ui.label("Payee");
        ui.text_edit_singleline(&mut form.payee);
        ui.end_row();
        ui.label("Tags");
        ui.add(egui::TextEdit::singleline(&mut form.tags).hint_text("comma, separated"));
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
                .hint_text("Markdown"),
        );
        ui.end_row();
    });

    ui.separator();
    ui.strong("Postings");
    let mut remove = None;
    // Default tags of the accounts chosen this frame.
    let mut picked: Vec<Vec<String>> = vec![];
    for (i, row) in form.postings.iter_mut().enumerate() {
        // Each posting wraps onto a second line when the pane is narrow.
        ui.horizontal_wrapped(|ui| {
            if let Some(a) = posting_row(ui, i, row, accounts) {
                picked.push(a.default_tags.clone());
            }
            if ui
                .add_enabled(i >= 2, egui::Button::new("×"))
                .on_hover_text("Remove this posting")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
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
}

/// A new account's fields.
fn account_fields(ui: &mut egui::Ui, form: &mut AccountForm) {
    egui::Grid::new("account_fields")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("Expenses:Food"));
            ui.end_row();
            ui.label("Type");
            egui::ComboBox::from_id_salt("account_type")
                .selected_text(form::type_name(form.account_type))
                .show_ui(ui, |ui| {
                    for ty in form::ACCOUNT_TYPES {
                        ui.selectable_value(&mut form.account_type, ty, form::type_name(ty));
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
            ui.add(egui::TextEdit::singleline(&mut form.default_tags).hint_text("living"));
            ui.end_row();
            ui.label("Opened");
            ui.add(egui::TextEdit::singleline(&mut form.opened_at).hint_text("YYYY-MM-DD"));
            ui.end_row();
            ui.label("Description");
            ui.add(
                egui::TextEdit::multiline(&mut form.description)
                    .desired_rows(3)
                    .hint_text("Markdown"),
            );
            ui.end_row();
        });
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

/// Whether the books still hold what `pick` names.
fn still_there(pick: &Pick, books: &Books) -> bool {
    match pick {
        Pick::Transaction(id) => books.postings.iter().any(|p| &p.transaction_id == id),
        Pick::Account(id) => books.accounts.iter().any(|a| &a.id == id),
    }
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

    fn view() -> (tokio::runtime::Runtime, LedgerView) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let endpoint = sapphire_framework::ipc::Endpoint::in_dir(
            "test",
            std::env::temp_dir().join("ledger-test"),
        );
        let client = Arc::new(LedgerClient::new("test", "0.0.0", endpoint));
        let view = LedgerView::new(runtime.handle().clone(), client, Arc::new(|| {}));
        (runtime, view)
    }

    fn books(accounts: Vec<Account>, postings: Vec<PostingHit>) -> Books {
        Books {
            accounts,
            postings,
            issues: vec![],
        }
    }

    #[test]
    fn a_pick_lasts_while_the_books_hold_it() {
        let b = books(
            vec![account("a", "Assets:Cash")],
            vec![hit(Some("a"), None, "1", "JPY")],
        );
        assert!(still_there(&Pick::Account("a".into()), &b));
        assert!(!still_there(&Pick::Account("b".into()), &b));
        assert!(still_there(&Pick::Transaction("tx00001".into()), &b));
        assert!(!still_there(&Pick::Transaction("tx00002".into()), &b));
    }

    #[test]
    fn a_refresh_drops_a_vanished_pick() {
        let (_rt, mut view) = view();
        view.picked = Some(Pick::Transaction("tx00002".into()));
        view.replies
            .0
            .send(Reply::Books(Ok(books(
                vec![account("a", "Assets:Cash")],
                vec![hit(Some("a"), None, "1", "JPY")],
            ))))
            .unwrap();
        assert!(!view.drain_replies());
        assert_eq!(view.picked, None);
    }

    #[test]
    fn a_reply_for_another_transaction_is_dropped() {
        let (_rt, mut view) = view();
        view.picked = Some(Pick::Transaction("tx00001".into()));
        view.replies
            .0
            .send(Reply::Transaction("tx00002".into(), Err("gone".into())))
            .unwrap();
        view.drain_replies();
        assert!(view.transaction.is_none());
        view.replies
            .0
            .send(Reply::Transaction("tx00001".into(), Err("gone".into())))
            .unwrap();
        view.drain_replies();
        assert!(view.transaction.is_some());
    }

    #[test]
    fn a_new_record_closes_the_pane_and_new_tags_return_to_the_account() {
        let (_rt, mut view) = view();
        view.editor = Some(Editor::Transaction(TxForm::new("2026-10-28".into(), "JPY")));
        view.replies
            .0
            .send(Reply::Written(Sent::Transaction, Ok("recorded".into())))
            .unwrap();
        assert!(view.drain_replies());
        assert!(view.editor.is_none());

        view.picked = Some(Pick::Account("a".into()));
        view.editor = Some(Editor::Tags(TagsForm::new(&account("a", "Assets:Cash"))));
        view.replies
            .0
            .send(Reply::Written(Sent::Tags, Ok("updated".into())))
            .unwrap();
        assert!(!view.drain_replies());
        assert!(view.editor.is_none());
        assert_eq!(view.picked, Some(Pick::Account("a".into())));
    }

    #[test]
    fn a_refused_write_keeps_the_form() {
        let (_rt, mut view) = view();
        view.editor = Some(Editor::Account(AccountForm::new("2026-10-28".into())));
        view.replies
            .0
            .send(Reply::Written(Sent::Account, Err("taken".into())))
            .unwrap();
        assert!(!view.drain_replies());
        assert!(matches!(view.editor, Some(Editor::Account(_))));
        assert_eq!(view.form_error.as_deref(), Some("taken"));
    }
}
