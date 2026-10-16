//! The Charts screen: one budget's month as the server's `budget_report`
//! answers it — the daily pace, and the balance path.
//!
//! Like the Ledger screen it is a client of the running server: calls run on
//! the tokio runtime and come back through a channel, and the view never
//! blocks. Everything it draws is computed by the server; the helpers here
//! only turn the answer into plot points and text.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use egui_plot::{Bar, BarChart, HLine, HoverPosition, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use sapphire_framework::gui::egui::{self, Color32};
use sapphire_ledger_api::{
    self as api, Account, Budget, BudgetDay, BudgetReport, BudgetReportParams, EmptyParams,
};

use crate::client::LedgerClient;
use crate::form::{self, BudgetForm};

/// As the Ledger screen: pick up what the CLI, an agent or sync wrote.
const REFRESH_EVERY: Duration = Duration::from_secs(10);

/// Numbers each load, so a reply for a budget or month no longer shown is
/// dropped instead of drawn.
#[derive(Default)]
struct Generation(u64);

impl Generation {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    fn is_current(&self, n: u64) -> bool {
        self.0 == n
    }
}

struct Loaded {
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    report: Option<BudgetReport>,
}

enum Reply {
    Loaded(u64, Result<Box<Loaded>, String>),
    Written(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Pace,
    Balance,
}

pub struct ChartsView {
    runtime: tokio::runtime::Handle,
    client: Arc<LedgerClient>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    replies: (mpsc::Sender<Reply>, mpsc::Receiver<Reply>),
    generation: Generation,

    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The budget shown, by id; `None` until the first load picks one.
    selected: Option<String>,
    month: (i32, u32),
    report: Option<BudgetReport>,
    tab: Tab,
    load_error: Option<String>,
    loading: bool,
    last_refresh: Option<Instant>,

    form: Option<BudgetForm>,
    sending: bool,
    form_error: Option<String>,
}

impl ChartsView {
    pub fn new(
        runtime: tokio::runtime::Handle,
        client: Arc<LedgerClient>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> ChartsView {
        let today = chrono::Local::now().date_naive();
        ChartsView {
            runtime,
            client,
            repaint,
            replies: mpsc::channel(),
            generation: Generation::default(),
            budgets: vec![],
            accounts: vec![],
            selected: None,
            month: (today.year(), today.month()),
            report: None,
            tab: Tab::Pace,
            load_error: None,
            loading: false,
            last_refresh: None,
            form: None,
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
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        self.toolbar(ui);
        ui.separator();
        match &self.report {
            None if self.budgets.is_empty() && !self.loading => {
                ui.label("No budgets yet. A budget counts the transactions carrying its tag: start with New budget.");
            }
            None => {
                ui.spinner();
            }
            Some(report) => {
                ui.label(summary(report));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Pace, "Daily pace");
                    ui.selectable_value(&mut self.tab, Tab::Balance, "Balance path");
                });
                let colors = Colors::for_mode(ui.visuals().dark_mode);
                match self.tab {
                    Tab::Pace => pace_plots(ui, report, colors),
                    Tab::Balance => match balance_series(report) {
                        Some(series) => balance_plot(ui, series, colors),
                        None => {
                            ui.label("This budget has no accounts it is paid from. Choose some with Edit to see its balance path.");
                        }
                    },
                }
            }
        }
        self.form_window(ui.ctx());
    }

    // ── server calls ────────────────────────────────────────────────────────

    fn refresh(&mut self) {
        self.loading = true;
        self.last_refresh = Some(Instant::now());
        let n = self.generation.next();
        let client = Arc::clone(&self.client);
        let selected = self.selected.clone();
        let month = month_label(self.month);
        self.spawn(async move {
            let loaded = async {
                let budgets: Vec<Budget> = client.read(api::LIST_BUDGETS, EmptyParams {}).await?;
                let accounts: Vec<Account> =
                    client.read(api::LIST_ACCOUNTS, EmptyParams {}).await?;
                let shown = selected
                    .filter(|id| budgets.iter().any(|b| &b.id == id))
                    .or_else(|| budgets.first().map(|b| b.id.clone()));
                let report = match shown {
                    Some(budget) => Some(
                        client
                            .read(api::BUDGET_REPORT, BudgetReportParams { budget, month })
                            .await?,
                    ),
                    None => None,
                };
                Ok(Box::new(Loaded {
                    budgets,
                    accounts,
                    report,
                }))
            };
            Reply::Loaded(n, loaded.await)
        });
    }

    /// Show another budget or month now, not at the next tick.
    fn reload_now(&mut self) {
        self.report = None;
        self.loading = false;
        self.last_refresh = None;
    }

    fn send_form(&mut self) {
        let Some(form) = &self.form else { return };
        let client = Arc::clone(&self.client);
        let call = match &form.id {
            None => form
                .to_add_params()
                .map(|p| (api::ADD_BUDGET, serde_json::to_value(p))),
            Some(_) => form
                .to_update_params()
                .map(|p| (api::UPDATE_BUDGET, serde_json::to_value(p))),
        };
        match call {
            Ok((method, Ok(params))) => {
                self.sending = true;
                self.form_error = None;
                self.spawn(async move { Reply::Written(client.call(method, params).await) });
            }
            Ok((_, Err(e))) => self.form_error = Some(e.to_string()),
            Err(e) => self.form_error = Some(e),
        }
    }

    fn spawn(&self, call: impl Future<Output = Reply> + Send + 'static) {
        let tx = self.replies.0.clone();
        let repaint = Arc::clone(&self.repaint);
        self.runtime.spawn(async move {
            let _ = tx.send(call.await);
            repaint();
        });
    }

    fn drain_replies(&mut self) {
        while let Ok(reply) = self.replies.1.try_recv() {
            match reply {
                Reply::Loaded(n, _) if !self.generation.is_current(n) => {}
                Reply::Loaded(_, Ok(loaded)) => {
                    self.selected = loaded.report.as_ref().map(|r| r.budget.id.clone());
                    self.budgets = loaded.budgets;
                    self.accounts = loaded.accounts;
                    self.report = loaded.report;
                    self.load_error = None;
                    self.loading = false;
                }
                Reply::Loaded(_, Err(e)) => {
                    // Keep the last chart: a server restart should not blank it.
                    self.load_error = Some(e);
                    self.loading = false;
                }
                Reply::Written(Ok(_)) => {
                    self.sending = false;
                    self.form = None;
                    self.reload_now();
                }
                Reply::Written(Err(e)) => {
                    self.sending = false;
                    self.form_error = Some(e);
                }
            }
        }
    }

    // ── panels ──────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Owned, so the arms below can call `&mut self` methods.
            let shown: Option<Budget> = self
                .budgets
                .iter()
                .find(|b| Some(&b.id) == self.selected.as_ref())
                .cloned();
            let mut picked = self.selected.clone();
            egui::ComboBox::from_id_salt("charts_budget")
                .selected_text(shown.as_ref().map_or("Budget…", |b| b.name.as_str()))
                .show_ui(ui, |ui| {
                    for b in &self.budgets {
                        ui.selectable_value(&mut picked, Some(b.id.clone()), &b.name);
                    }
                });
            let mut month = self.month;
            if ui
                .small_button("◀")
                .on_hover_text("Previous month")
                .clicked()
            {
                month = shift_month(month, -1);
            }
            ui.monospace(month_label(month));
            if ui.small_button("▶").on_hover_text("Next month").clicked() {
                month = shift_month(month, 1);
            }
            if picked != self.selected || month != self.month {
                self.selected = picked;
                self.month = month;
                self.reload_now();
            }
            let can_write = self.load_error.is_none();
            if ui
                .add_enabled(can_write, egui::Button::new("New budget"))
                .clicked()
            {
                let currency = shown
                    .as_ref()
                    .map_or("JPY", |b| b.currency.as_str())
                    .to_owned();
                self.form = Some(BudgetForm::new(&currency));
                self.form_error = None;
            }
            if ui
                .add_enabled(can_write && shown.is_some(), egui::Button::new("Edit"))
                .clicked()
                && let Some(b) = &shown
            {
                self.form = Some(BudgetForm::edit(b, &self.accounts));
                self.form_error = None;
            }
            if self.loading {
                ui.spinner();
            }
        });
        if let Some(e) = &self.load_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn form_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.form else { return };
        let mut assets: Vec<&Account> = self
            .accounts
            .iter()
            .filter(|a| a.account_type == api::AccountType::Asset)
            .collect();
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        let title = if form.id.is_some() {
            "Edit budget"
        } else {
            "New budget"
        };
        let (mut open, mut submit) = (true, false);
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("budget_fields")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("生活費"));
                        ui.end_row();
                        ui.label("Tag");
                        ui.add(egui::TextEdit::singleline(&mut form.tag).hint_text("living"));
                        ui.end_row();
                        ui.label("Currency");
                        ui.add(egui::TextEdit::singleline(&mut form.currency).desired_width(60.0));
                        ui.end_row();
                        ui.label("Per month");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.monthly_amount)
                                .hint_text("100000"),
                        );
                        ui.end_row();
                    });
                ui.separator();
                ui.label("Paid from (for the balance path)");
                if assets.is_empty() {
                    ui.weak("No asset accounts yet.");
                }
                for a in &assets {
                    let mut on = form.balance_accounts.contains(&a.id);
                    if ui.checkbox(&mut on, &a.name).changed() {
                        form.toggle(&a.id);
                    }
                }
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
            self.form = None;
            self.form_error = None;
        } else if submit {
            self.send_form();
        }
    }
}

// ── plots ───────────────────────────────────────────────────────────────────

/// Actual and plan, stepped per theme; validated as a pair with the dataviz
/// validator in both modes. Plan is also dashed, so colour is never alone.
#[derive(Clone, Copy)]
struct Colors {
    actual: Color32,
    plan: Color32,
}

impl Colors {
    fn for_mode(dark: bool) -> Colors {
        if dark {
            Colors {
                actual: Color32::from_rgb(0x39, 0x87, 0xe5),
                plan: Color32::from_rgb(0xd9, 0x59, 0x26),
            }
        } else {
            Colors {
                actual: Color32::from_rgb(0x2a, 0x78, 0xd6),
                plan: Color32::from_rgb(0xeb, 0x68, 0x34),
            }
        }
    }
}

/// A plot of one month: day 1..=n on x, nothing to pan or zoom.
fn month_plot<'a>(id: &'a str, days: usize) -> Plot<'a> {
    Plot::new(id)
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_x(0.5)
        .include_x(days as f64 + 0.5)
        .include_y(0.0)
        .link_axis("budget_month", [true, false])
        .link_cursor("budget_month", [true, false])
        .x_axis_formatter(|mark, _| format!("{}", mark.value.round() as i64))
        .y_axis_formatter(|mark, _| grouped(&format!("{}", mark.value.round() as i64)))
        .label_formatter(|pos: &HoverPosition<'_>| match pos {
            HoverPosition::NearDataPoint {
                plot_name,
                position,
                ..
            } => Some(format!(
                "{plot_name}\nday {}: {}",
                position.x.round() as i64,
                grouped(&format!("{:.2}", position.y)),
            )),
            HoverPosition::Elsewhere { .. } => None,
        })
}

fn pace_plots(ui: &mut egui::Ui, report: &BudgetReport, c: Colors) {
    let s = pace_series(report);
    let n = report.days.len();
    let tall = (ui.available_height() * 0.62).max(160.0);
    month_plot("budget_pace", n).height(tall).show(ui, |p| {
        p.line(
            Line::new("Budget so far", PlotPoints::from(s.budget))
                .color(c.plan)
                .width(2.0)
                .style(LineStyle::dashed_loose()),
        );
        p.line(
            Line::new("Spent so far", PlotPoints::from(s.spent))
                .color(c.actual)
                .width(2.0),
        );
    });
    month_plot("budget_daily", n)
        .height(ui.available_height().max(100.0))
        .show(ui, |p| {
            let bars = s
                .daily
                .iter()
                .map(|[x, y]| Bar::new(*x, *y).width(0.7))
                .collect();
            p.bar_chart(BarChart::new("Spent that day", bars).color(c.actual));
            p.hline(
                HLine::new("Daily budget", s.daily_budget)
                    .color(c.plan)
                    .width(2.0)
                    .style(LineStyle::dashed_loose()),
            );
        });
}

fn balance_plot(ui: &mut egui::Ui, s: BalanceSeries, c: Colors) {
    month_plot("budget_balance", s.planned.len())
        .height(ui.available_height().max(160.0))
        .show(ui, |p| {
            p.line(
                Line::new("Planned balance", PlotPoints::from(s.planned))
                    .color(c.plan)
                    .width(2.0)
                    .style(LineStyle::dashed_loose()),
            );
            p.line(
                Line::new("Actual balance", PlotPoints::from(s.actual))
                    .color(c.actual)
                    .width(2.0),
            );
        });
}

// ── pure helpers ────────────────────────────────────────────────────────────

struct PaceSeries {
    budget: Vec<[f64; 2]>,
    /// Elapsed days only: the line stops at today.
    spent: Vec<[f64; 2]>,
    daily: Vec<[f64; 2]>,
    daily_budget: f64,
}

struct BalanceSeries {
    planned: Vec<[f64; 2]>,
    actual: Vec<[f64; 2]>,
}

fn number(raw: &str) -> f64 {
    form::parse_amount(raw)
        .and_then(|d| d.to_f64())
        .unwrap_or_default()
}

fn x(d: &BudgetDay) -> f64 {
    f64::from(d.date.day())
}

fn pace_series(r: &BudgetReport) -> PaceSeries {
    let elapsed = || r.days.iter().filter(|d| d.elapsed);
    PaceSeries {
        budget: r
            .days
            .iter()
            .map(|d| [x(d), number(&d.budget_cumulative)])
            .collect(),
        spent: elapsed()
            .map(|d| [x(d), number(&d.spent_cumulative)])
            .collect(),
        daily: elapsed().map(|d| [x(d), number(&d.spent)]).collect(),
        daily_budget: number(&r.daily_budget),
    }
}

/// `None` when nothing has an actual balance: no accounts to draw.
fn balance_series(r: &BudgetReport) -> Option<BalanceSeries> {
    let actual: Vec<[f64; 2]> = r
        .days
        .iter()
        .filter_map(|d| d.actual_balance.as_deref().map(|a| [x(d), number(a)]))
        .collect();
    if actual.is_empty() && r.budget.balance_accounts.is_empty() {
        return None;
    }
    Some(BalanceSeries {
        planned: r
            .days
            .iter()
            .map(|d| [x(d), number(&d.planned_balance)])
            .collect(),
        actual,
    })
}

/// `9,000 / 100,000 JPY · 3,225.81 / day · 2,548.39 over pace`. The pace
/// part is left out until a day has elapsed.
fn summary(r: &BudgetReport) -> String {
    let last = r.days.iter().rev().find(|d| d.elapsed);
    let spent = last.map_or("0", |d| d.spent_cumulative.as_str());
    let mut text = format!(
        "{} / {} {} · {} / day",
        grouped(spent),
        grouped(&r.budget.monthly_amount),
        r.budget.currency,
        grouped(&r.daily_budget),
    );
    if let Some(d) = last {
        let ahead = form::parse_amount(&d.budget_cumulative).unwrap_or_default()
            - form::parse_amount(&d.spent_cumulative).unwrap_or_default();
        let word = if ahead < Decimal::ZERO {
            "over"
        } else {
            "under"
        };
        text.push_str(&format!(
            " · {} {word} pace",
            grouped(&ahead.abs().normalize().to_string())
        ));
    }
    text
}

/// `-1234567.5` → `-1,234,567.5`.
fn grouped(raw: &str) -> String {
    let (sign, digits) = raw.strip_prefix('-').map_or(("", raw), |d| ("-", d));
    let (int, frac) = digits
        .split_once('.')
        .map_or((digits, None), |(i, f)| (i, Some(f)));
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

fn shift_month((year, month): (i32, u32), by: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + by;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

fn month_label((year, month): (i32, u32)) -> String {
    format!("{year:04}-{month:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(
        n: u32,
        spent: &str,
        spent_total: &str,
        budget_total: &str,
        elapsed: bool,
        actual: Option<&str>,
    ) -> BudgetDay {
        BudgetDay {
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, n).unwrap(),
            spent: spent.into(),
            spent_cumulative: spent_total.into(),
            budget_cumulative: budget_total.into(),
            planned_balance: "0".into(),
            elapsed,
            actual_balance: actual.map(Into::into),
        }
    }

    fn report(days: Vec<BudgetDay>) -> BudgetReport {
        BudgetReport {
            budget: Budget {
                id: "b".into(),
                name: "生活費".into(),
                tag: "living".into(),
                currency: "JPY".into(),
                monthly_amount: "100000".into(),
                balance_accounts: vec![],
            },
            month: "2026-10".into(),
            daily_budget: "3225.81".into(),
            opening_balance: "0".into(),
            days,
        }
    }

    #[test]
    fn the_actual_lines_stop_at_the_last_elapsed_day() {
        let r = report(vec![
            day(1, "1000", "1000", "3225.81", true, Some("-1000")),
            day(2, "0", "1000", "6451.61", true, Some("-1000")),
            day(3, "0", "1000", "9677.42", false, None),
        ]);
        let s = pace_series(&r);
        assert_eq!(s.budget.len(), 3);
        assert_eq!(s.spent, vec![[1.0, 1000.0], [2.0, 1000.0]]);
        assert_eq!(s.daily, vec![[1.0, 1000.0], [2.0, 0.0]]);
        assert!((s.daily_budget - 3225.81).abs() < 1e-9);
        let b = balance_series(&r).expect("there are actual balances");
        assert_eq!(b.actual.len(), 2);
        assert_eq!(b.planned.len(), 3);
    }

    #[test]
    fn no_actual_balance_means_no_balance_chart() {
        let r = report(vec![day(1, "0", "0", "3225.81", true, None)]);
        assert!(balance_series(&r).is_none());
    }

    #[test]
    fn the_summary_says_where_today_stands() {
        let r = report(vec![
            day(1, "1000", "1000", "3225.81", true, None),
            day(2, "8000", "9000", "6451.61", true, None),
            day(3, "0", "9000", "9677.42", false, None),
        ]);
        assert_eq!(
            summary(&r),
            "9,000 / 100,000 JPY · 3,225.81 / day · 2,548.39 over pace"
        );
        let r = report(vec![day(1, "0", "0", "3225.81", false, None)]);
        assert_eq!(summary(&r), "0 / 100,000 JPY · 3,225.81 / day");
    }

    #[test]
    fn thousands_are_grouped_and_months_roll_over() {
        assert_eq!(grouped("-1234567.5"), "-1,234,567.5");
        assert_eq!(grouped("999"), "999");
        assert_eq!(shift_month((2026, 12), 1), (2027, 1));
        assert_eq!(shift_month((2026, 1), -1), (2025, 12));
        assert_eq!(month_label((2026, 3)), "2026-03");
    }

    #[test]
    fn stale_replies_are_ignored() {
        let mut generation = Generation::default();
        let old = generation.next();
        let new = generation.next();
        assert!(!generation.is_current(old));
        assert!(generation.is_current(new));
    }
}
