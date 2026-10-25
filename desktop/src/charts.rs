//! The Charts screen: the workspace's panels, each drawn from the server's
//! `run_panel`, and the budgets a panel can draw against.
//!
//! Like the Ledger screen it is a client of the running server: calls run on
//! the tokio runtime and come back through a channel, and the view never
//! blocks. Everything it draws is computed by the server; the helpers here
//! only turn the answer into plot points and text.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use egui_plot::{Bar, BarChart, HoverPosition, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_decimal::prelude::ToPrimitive;
use sapphire_framework::gui::egui::{self, Color32};
use sapphire_ledger_api::{
    self as api, Account, AccountType, AddPanelParams, Budget, BudgetLine, DeletePanelParams,
    EmptyParams, GroupBy, Interval, Mark, Measure, Panel, PanelRun, Period, PlotRun,
    RunPanelParams, UpdatePanelParams,
};

use crate::client::LedgerClient;
use crate::form::{self, BudgetForm, PanelForm, PlotRow, SeriesRow, Template};

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
    panels: Vec<Panel>,
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The panel the run is of.
    shown: Option<String>,
    /// The panel's run; its error is shown in the body, not as a load error.
    run: Option<Result<PanelRun, String>>,
}

/// Which form a write came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Budget,
    Panel,
}

enum Reply {
    Loaded(u64, Result<Box<Loaded>, String>),
    /// A write from `Source` finished; on success, select the panel of this
    /// name next.
    Written(Source, Option<String>, Result<String, String>),
    /// The run of an unsaved definition.
    Preview(u64, Result<PanelRun, String>),
    Deleted(Result<String, String>),
}

/// What a row's `↑ ↓ ×` buttons asked for.
#[derive(Clone, Copy)]
enum Move {
    Up,
    Down,
    Remove,
}

impl Move {
    fn apply<T>(self, items: &mut Vec<T>, i: usize) {
        match self {
            Move::Up => form::move_item(items, i, true),
            Move::Down => form::move_item(items, i, false),
            Move::Remove => {
                items.remove(i);
            }
        }
    }
}

pub struct ChartsView {
    runtime: tokio::runtime::Handle,
    client: Arc<LedgerClient>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    replies: (mpsc::Sender<Reply>, mpsc::Receiver<Reply>),
    generation: Generation,

    panels: Vec<Panel>,
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The panel shown, by id.
    selected: Option<String>,
    /// After a write: the panel to pick on the next load, by name.
    select_name: Option<String>,
    /// Periods from the current one.
    offset: i32,
    run: Option<Result<PanelRun, String>>,
    load_error: Option<String>,
    loading: bool,
    last_refresh: Option<Instant>,

    budgets_open: bool,
    budget_form: Option<BudgetForm>,
    sending: bool,
    form_error: Option<String>,

    editor: Option<PanelForm>,
    template: Template,
    /// The budget a budget template is built from, by id.
    template_budget: Option<String>,
    /// The run shown is of the editor's unsaved definition.
    previewing: bool,
    confirm_delete: Option<Panel>,
    /// A delete is sent and not answered yet.
    deleting: bool,
}

/// Whether the body says there are no panels: not while an unsaved
/// definition is previewed.
fn shows_placeholder(no_panels: bool, loading: bool, previewing: bool) -> bool {
    no_panels && !loading && !previewing
}

/// Panels in the order the list shows them: by name.
fn sort_by_name(panels: &mut [Panel]) {
    panels.sort_by(|a, b| a.name.cmp(&b.name));
}

/// The panel to show: the selected one if it is still there, else the one
/// just written by name, else the first (`panels` is sorted by name).
fn pick(panels: &[Panel], selected: Option<&str>, by_name: Option<&str>) -> Option<String> {
    by_name
        .and_then(|name| panels.iter().find(|p| p.name == name))
        .or_else(|| selected.and_then(|id| panels.iter().find(|p| p.id == id)))
        .or_else(|| panels.first())
        .map(|p| p.id.clone())
}

impl ChartsView {
    pub fn new(
        runtime: tokio::runtime::Handle,
        client: Arc<LedgerClient>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> ChartsView {
        ChartsView {
            runtime,
            client,
            repaint,
            replies: mpsc::channel(),
            generation: Generation::default(),
            panels: vec![],
            budgets: vec![],
            accounts: vec![],
            selected: None,
            select_name: None,
            offset: 0,
            run: None,
            load_error: None,
            loading: false,
            last_refresh: None,
            budgets_open: false,
            budget_form: None,
            sending: false,
            form_error: None,
            editor: None,
            template: Template::ALL[0],
            template_budget: None,
            previewing: false,
            confirm_delete: None,
            deleting: false,
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

        egui::Panel::left("charts_panels")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| self.panel_list(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            self.toolbar(ui);
            ui.separator();
            self.body(ui);
        });
        self.budgets_window(ui.ctx());
        self.budget_form_window(ui.ctx());
        self.editor_window(ui.ctx());
        self.delete_window(ui.ctx());
    }

    // ── server calls ────────────────────────────────────────────────────────

    fn refresh(&mut self) {
        // The preview stays until Save or Close.
        if self.previewing {
            return;
        }
        self.loading = true;
        self.last_refresh = Some(Instant::now());
        let n = self.generation.next();
        let client = Arc::clone(&self.client);
        let selected = self.selected.clone();
        let select_name = self.select_name.clone();
        let offset = self.offset;
        self.spawn(async move {
            let loaded = async {
                let mut panels: Vec<Panel> = client.read(api::LIST_PANELS, EmptyParams {}).await?;
                sort_by_name(&mut panels);
                let budgets: Vec<Budget> = client.read(api::LIST_BUDGETS, EmptyParams {}).await?;
                let accounts: Vec<Account> =
                    client.read(api::LIST_ACCOUNTS, EmptyParams {}).await?;
                let shown = pick(&panels, selected.as_deref(), select_name.as_deref());
                let run = match &shown {
                    Some(id) => Some(
                        client
                            .read::<PanelRun>(
                                api::RUN_PANEL,
                                RunPanelParams {
                                    panel: Some(id.clone()),
                                    definition: None,
                                    anchor: None,
                                    offset: Some(offset),
                                },
                            )
                            .await,
                    ),
                    None => None,
                };
                Ok(Box::new(Loaded {
                    panels,
                    budgets,
                    accounts,
                    shown,
                    run,
                }))
            };
            Reply::Loaded(n, loaded.await)
        });
    }

    /// Show another panel or period now, not at the next tick.
    fn reload_now(&mut self) {
        // A load already in flight is for the old panel or period.
        self.generation.next();
        self.run = None;
        self.loading = false;
        self.last_refresh = None;
    }

    fn send_form(&mut self) {
        let Some(form) = &self.budget_form else {
            return;
        };
        let client = Arc::clone(&self.client);
        let select =
            (form.id.is_none() && form.create_panels).then(|| format!("{} pace", form.name.trim()));
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
                self.spawn(async move {
                    Reply::Written(Source::Budget, select, client.call(method, params).await)
                });
            }
            Ok((_, Err(e))) => self.form_error = Some(e.to_string()),
            Err(e) => self.form_error = Some(e),
        }
    }

    /// Run the editor's unsaved definition.
    fn preview(&mut self) {
        let Some(form) = &self.editor else {
            return;
        };
        match form.to_definition() {
            Ok(definition) => {
                self.form_error = None;
                self.previewing = true;
                // A load in flight is overtaken; its reply will not reset this.
                self.loading = false;
                let n = self.generation.next();
                let client = Arc::clone(&self.client);
                let offset = self.offset;
                self.spawn(async move {
                    Reply::Preview(
                        n,
                        client
                            .read::<PanelRun>(
                                api::RUN_PANEL,
                                RunPanelParams {
                                    panel: None,
                                    definition: Some(definition),
                                    anchor: None,
                                    offset: Some(offset),
                                },
                            )
                            .await,
                    )
                });
            }
            Err(e) => self.form_error = Some(e),
        }
    }

    fn save_panel(&mut self) {
        let Some(form) = &self.editor else {
            return;
        };
        let definition = match form.to_definition() {
            Ok(d) => d,
            Err(e) => {
                self.form_error = Some(e);
                return;
            }
        };
        let client = Arc::clone(&self.client);
        let select = Some(definition.name.clone());
        let call = match &form.id {
            None => {
                serde_json::to_value(AddPanelParams { definition }).map(|p| (api::ADD_PANEL, p))
            }
            Some(panel) => serde_json::to_value(UpdatePanelParams {
                panel: panel.clone(),
                definition,
            })
            .map(|p| (api::UPDATE_PANEL, p)),
        };
        match call {
            Ok((method, params)) => {
                self.sending = true;
                self.form_error = None;
                self.spawn(async move {
                    Reply::Written(Source::Panel, select, client.call(method, params).await)
                });
            }
            Err(e) => self.form_error = Some(e.to_string()),
        }
    }

    fn delete_panel(&mut self, panel: String) {
        self.deleting = true;
        let client = Arc::clone(&self.client);
        self.spawn(async move {
            Reply::Deleted(match serde_json::to_value(DeletePanelParams { panel }) {
                Ok(params) => client.call(api::DELETE_PANEL, params).await,
                Err(e) => Err(e.to_string()),
            })
        });
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
                    let loaded = *loaded;
                    self.panels = loaded.panels;
                    sort_by_name(&mut self.panels);
                    self.budgets = loaded.budgets;
                    self.accounts = loaded.accounts;
                    self.selected = loaded.shown;
                    self.run = loaded.run;
                    self.select_name = None;
                    self.load_error = None;
                    self.loading = false;
                }
                Reply::Loaded(_, Err(e)) => {
                    // Keep the last chart: a server restart should not blank it.
                    self.load_error = Some(e);
                    self.loading = false;
                }
                Reply::Preview(n, _) if !self.generation.is_current(n) => {}
                Reply::Preview(_, run) => self.run = Some(run),
                Reply::Deleted(Ok(_)) => {
                    self.deleting = false;
                    self.selected = None;
                    let deleted = self.confirm_delete.take();
                    if let (Some(p), Some(form)) = (&deleted, &self.editor)
                        && form.id.as_deref() == Some(p.id.as_str())
                    {
                        self.editor = None;
                    }
                    self.previewing = false;
                    self.reload_now();
                }
                Reply::Deleted(Err(e)) => {
                    self.deleting = false;
                    self.form_error = Some(e);
                }
                Reply::Written(source, select, Ok(_)) => {
                    self.sending = false;
                    match source {
                        Source::Budget => self.budget_form = None,
                        Source::Panel => {
                            self.editor = None;
                            self.previewing = false;
                        }
                    }
                    self.select_name = select;
                    if self.select_name.is_some() {
                        self.offset = 0;
                    }
                    self.reload_now();
                }
                Reply::Written(_, _, Err(e)) => {
                    self.sending = false;
                    self.form_error = Some(e);
                }
            }
        }
    }

    // ── screen parts ────────────────────────────────────────────────────────

    fn panel_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Panels");
        let mut clicked = None;
        for p in &self.panels {
            let on = self.selected.as_deref() == Some(p.id.as_str());
            if ui.selectable_label(on, &p.name).clicked() {
                clicked = Some(p.id.clone());
            }
        }
        if self.panels.is_empty() {
            ui.weak("No panels yet.");
        }
        if let Some(id) = clicked {
            self.selected = Some(id);
            // A name pending from a write that failed to load would override the click.
            self.select_name = None;
            self.offset = 0;
            self.previewing = false;
            self.reload_now();
        }
        ui.separator();
        if ui.button("New panel").clicked() {
            self.editor = Some(PanelForm::new());
            self.form_error = None;
        }
        if ui.button("Budgets…").clicked() {
            self.budgets_open = true;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let shown = self
                .panels
                .iter()
                .find(|p| self.selected.as_deref() == Some(p.id.as_str()))
                .cloned();
            if let Some(p) = &shown {
                ui.strong(&p.name);
            }
            if self.previewing {
                ui.weak("Preview — not saved");
            }
            let mut offset = self.offset;
            if ui
                .small_button("◀")
                .on_hover_text("Previous period")
                .clicked()
            {
                offset -= 1;
            }
            match &self.run {
                Some(Ok(run)) => ui.monospace(period_label(run)),
                _ => ui.monospace(""),
            };
            if ui.small_button("▶").on_hover_text("Next period").clicked() {
                offset += 1;
            }
            if offset != self.offset {
                self.offset = offset;
                if self.previewing {
                    self.preview();
                } else {
                    self.reload_now();
                }
            }
            if self.loading {
                ui.spinner();
            }
            if ui
                .add_enabled(shown.is_some(), egui::Button::new("Edit"))
                .clicked()
                && let Some(p) = &shown
            {
                self.editor = Some(PanelForm::from_definition(
                    Some(p.id.clone()),
                    &p.definition(),
                ));
                self.form_error = None;
            }
            if ui
                .add_enabled(shown.is_some(), egui::Button::new("Delete"))
                .clicked()
            {
                self.confirm_delete = shown.clone();
                self.form_error = None;
            }
        });
        if let Some(e) = &self.load_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        if shows_placeholder(self.panels.is_empty(), self.loading, self.previewing) {
            ui.label("No panels yet. Make one from a budget under Budgets…, or with an agent's add_panel.");
            return;
        }
        match &self.run {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Some(Ok(run)) => draw_run(ui, run),
        }
    }

    fn delete_window(&mut self, ctx: &egui::Context) {
        let Some(panel) = &self.confirm_delete else {
            return;
        };
        let (mut delete, mut cancel) = (false, false);
        egui::Window::new("Delete panel")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Delete panel {}? Its file is removed.", panel.name));
                if let Some(e) = &self.form_error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    delete = ui
                        .add_enabled(!self.deleting, egui::Button::new("Delete"))
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if delete {
            let id = panel.id.clone();
            self.form_error = None;
            self.delete_panel(id);
        } else if cancel {
            self.confirm_delete = None;
            self.form_error = None;
        }
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.editor.take() else {
            return;
        };
        let choices = form::account_choices(&self.accounts);
        let title = if form.id.is_some() {
            "Edit panel"
        } else {
            "New panel"
        };
        let (mut open, mut preview, mut save) = (true, false, false);
        egui::Window::new(title)
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        self.template_row(ui, &mut form);
                        ui.separator();
                        panel_fields(ui, &mut form);
                        plot_rows(ui, &mut form, &self.budgets, &self.accounts, &choices);
                        if ui.button("Add plot").clicked() {
                            form.plots.push(PlotRow::new());
                        }
                    });
                ui.separator();
                if let Some(e) = &self.form_error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    preview = ui
                        .add_enabled(!self.sending, egui::Button::new("Preview"))
                        .clicked();
                    save = ui
                        .add_enabled(!self.sending, egui::Button::new("Save"))
                        .clicked();
                    if self.sending {
                        ui.spinner();
                    }
                });
            });
        if !open {
            self.form_error = None;
            if self.previewing {
                self.previewing = false;
                self.reload_now();
            }
            return;
        }
        self.editor = Some(form);
        if preview {
            self.preview();
        } else if save {
            self.save_panel();
        }
    }

    fn template_row(&mut self, ui: &mut egui::Ui, form: &mut PanelForm) {
        ui.horizontal(|ui| {
            ui.label("Template");
            egui::ComboBox::from_id_salt("panel_template")
                .selected_text(self.template.label())
                .show_ui(ui, |ui| {
                    for t in Template::ALL {
                        ui.selectable_value(&mut self.template, t, t.label());
                    }
                });
            if self.template.needs_budget() {
                let chosen = self
                    .budgets
                    .iter()
                    .find(|b| self.template_budget.as_deref() == Some(b.id.as_str()))
                    .map_or("budget", |b| b.name.as_str());
                egui::ComboBox::from_id_salt("template_budget")
                    .selected_text(chosen)
                    .show_ui(ui, |ui| {
                        for b in &self.budgets {
                            ui.selectable_value(
                                &mut self.template_budget,
                                Some(b.id.clone()),
                                &b.name,
                            );
                        }
                    });
            }
            if ui.button("Use").clicked() {
                let budget = self
                    .budgets
                    .iter()
                    .find(|b| self.template_budget.as_deref() == Some(b.id.as_str()));
                match self.template.build(budget) {
                    Some(def) => {
                        *form = PanelForm::from_definition(form.id.clone(), &def);
                        self.form_error = None;
                    }
                    None => {
                        self.form_error = Some(
                            "choose a budget (the balance template also needs one paid from accounts)"
                                .into(),
                        );
                    }
                }
            }
        });
    }

    fn budgets_window(&mut self, ctx: &egui::Context) {
        if !self.budgets_open {
            return;
        }
        let mut open = true;
        let mut edit = None;
        let mut new = false;
        egui::Window::new("Budgets")
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                if self.budgets.is_empty() {
                    ui.weak("No budgets yet.");
                }
                for b in &self.budgets {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{} · {} {} / month",
                            b.name,
                            grouped(&b.monthly_amount),
                            b.currency
                        ));
                        if ui.button("Edit").clicked() {
                            edit = Some(b.clone());
                        }
                    });
                }
                ui.separator();
                new = ui.button("New budget").clicked();
            });
        self.budgets_open = open;
        if let Some(b) = edit {
            self.budget_form = Some(BudgetForm::edit(&b, &self.accounts));
            self.form_error = None;
        } else if new {
            let currency = self
                .budgets
                .first()
                .map_or("JPY", |b| b.currency.as_str())
                .to_owned();
            self.budget_form = Some(BudgetForm::new(&currency));
            self.form_error = None;
        }
    }

    fn budget_form_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.budget_form else {
            return;
        };
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
                if form.id.is_none() {
                    ui.checkbox(
                        &mut form.create_panels,
                        "Create its panels (pace, and balance when paid from accounts)",
                    );
                }
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
            self.budget_form = None;
            self.form_error = None;
        } else if submit {
            self.send_form();
        }
    }
}

// ── the panel editor ────────────────────────────────────────────────────────

fn period_name(p: Period) -> &'static str {
    match p {
        Period::Month => "Month",
        Period::Year => "Year",
        Period::Last12Months => "Last 12 months",
    }
}

fn interval_name(i: Option<Interval>) -> &'static str {
    match i {
        None => "Default",
        Some(Interval::Day) => "Day",
        Some(Interval::Week) => "Week",
        Some(Interval::Month) => "Month",
        Some(Interval::Year) => "Year",
    }
}

fn measure_name(m: Measure) -> &'static str {
    match m {
        Measure::Flow => "Flow",
        Measure::Balance => "Balance",
        Measure::Budget => "Budget",
    }
}

fn group_name(g: GroupBy) -> &'static str {
    match g {
        GroupBy::None => "None",
        GroupBy::Account => "Account",
        GroupBy::AccountType => "Account type",
        GroupBy::Tag => "Tag",
    }
}

fn line_name(l: BudgetLine) -> &'static str {
    match l {
        BudgetLine::Pace => "Pace",
        BudgetLine::Remaining => "Remaining",
    }
}

fn mark_name(m: Mark) -> &'static str {
    match m {
        Mark::Line => "Line",
        Mark::Bar => "Bar",
    }
}

/// A combo box over `options`, each shown by `name`.
fn pick_one<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut T,
    options: &[T],
    name: fn(T) -> &'static str,
) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name(*value))
        .show_ui(ui, |ui| {
            for o in options {
                ui.selectable_value(value, *o, name(*o));
            }
        });
}

/// `↑ ↓ ×` for one row of a list of `len`.
fn move_buttons(ui: &mut egui::Ui, len: usize) -> Option<Move> {
    let mut action = None;
    if ui.small_button("↑").on_hover_text("Move up").clicked() {
        action = Some(Move::Up);
    }
    if ui.small_button("↓").on_hover_text("Move down").clicked() {
        action = Some(Move::Down);
    }
    if ui
        .add_enabled(len > 1, egui::Button::new("×").small())
        .on_hover_text("Remove")
        .clicked()
    {
        action = Some(Move::Remove);
    }
    action
}

fn panel_fields(ui: &mut egui::Ui, form: &mut PanelForm) {
    egui::Grid::new("panel_fields")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
            ui.label("Period");
            pick_one(
                ui,
                "panel_period",
                &mut form.period,
                &[Period::Month, Period::Year, Period::Last12Months],
                period_name,
            );
            ui.end_row();
            ui.label("Interval");
            pick_one(
                ui,
                "panel_interval",
                &mut form.interval,
                &[
                    None,
                    Some(Interval::Day),
                    Some(Interval::Week),
                    Some(Interval::Month),
                    Some(Interval::Year),
                ],
                interval_name,
            );
            ui.end_row();
            ui.label("Currency");
            ui.add(egui::TextEdit::singleline(&mut form.currency).hint_text("base"));
            ui.end_row();
        });
}

fn plot_rows(
    ui: &mut egui::Ui,
    form: &mut PanelForm,
    budgets: &[Budget],
    accounts: &[Account],
    choices: &[String],
) {
    let plot_count = form.plots.len();
    let mut plot_move = None;
    for (pi, plot) in form.plots.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("Plot {}", pi + 1));
                if let Some(m) = move_buttons(ui, plot_count) {
                    plot_move = Some((pi, m));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Title");
                ui.text_edit_singleline(&mut plot.title);
                ui.checkbox(&mut plot.stacked, "Stacked");
            });
            let series_count = plot.series.len();
            let mut series_move = None;
            for (si, row) in plot.series.iter_mut().enumerate() {
                ui.group(|ui| {
                    if let Some(m) =
                        series_row(ui, (pi, si), row, series_count, budgets, accounts, choices)
                    {
                        series_move = Some((si, m));
                    }
                });
            }
            if let Some((si, m)) = series_move {
                m.apply(&mut plot.series, si);
            }
            if ui.button("Add series").clicked() {
                plot.series.push(SeriesRow::new(Measure::Flow));
            }
        });
    }
    if let Some((pi, m)) = plot_move {
        m.apply(&mut form.plots, pi);
    }
}

/// The `accounts` entries that are not one of the menu's `choices`.
fn unlisted_entries(entries: &[String], choices: &[String]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| !choices.contains(e))
        .cloned()
        .collect()
}

/// An unlisted entry as the menu shows it: the account it is the id of, with
/// the id, else the entry itself.
fn entry_label(entry: &str, accounts: &[Account]) -> String {
    match accounts.iter().find(|a| a.id == entry) {
        Some(a) => format!("{} (id {entry})", a.name),
        None => entry.to_owned(),
    }
}

/// Whether a series' `budget` field (an id or a name) names `b`.
fn is_budget(b: &Budget, field: &str) -> bool {
    b.id == field || b.name == field
}

/// The budget combo's selected text.
fn budget_text<'a>(budgets: &'a [Budget], field: Option<&str>) -> &'a str {
    field
        .and_then(|f| budgets.iter().find(|b| is_budget(b, f)))
        .map_or("choose", |b| b.name.as_str())
}

fn series_row(
    ui: &mut egui::Ui,
    at: (usize, usize),
    row: &mut SeriesRow,
    count: usize,
    budgets: &[Budget],
    accounts: &[Account],
    choices: &[String],
) -> Option<Move> {
    let mut action = None;
    ui.horizontal(|ui| {
        pick_one(
            ui,
            ("series_measure", at),
            &mut row.measure,
            &[Measure::Flow, Measure::Balance, Measure::Budget],
            measure_name,
        );
        action = move_buttons(ui, count);
    });
    match row.measure {
        Measure::Flow | Measure::Balance => {
            ui.horizontal_wrapped(|ui| {
                ui.menu_button(format!("Accounts ({})", row.accounts.len()), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for name in choices {
                                let mut on = row.accounts.contains(name);
                                if ui.checkbox(&mut on, name).changed() {
                                    if on {
                                        row.accounts.push(name.clone());
                                    } else {
                                        row.accounts.retain(|a| a != name);
                                    }
                                }
                            }
                            // Entries the menu does not list, such as account
                            // ids: shown checked so they can be removed.
                            for entry in unlisted_entries(&row.accounts, choices) {
                                let mut on = true;
                                if ui
                                    .checkbox(&mut on, entry_label(&entry, accounts))
                                    .changed()
                                {
                                    row.accounts.retain(|a| *a != entry);
                                }
                            }
                        });
                });
                for ty in [
                    AccountType::Asset,
                    AccountType::Liability,
                    AccountType::Equity,
                    AccountType::Income,
                    AccountType::Expense,
                ] {
                    let mut on = row.account_types.contains(&ty);
                    if ui.checkbox(&mut on, form::type_name(ty)).changed() {
                        if on {
                            row.account_types.push(ty);
                        } else {
                            row.account_types.retain(|t| *t != ty);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Tags");
                ui.add(egui::TextEdit::singleline(&mut row.tags).hint_text("living, food"));
            });
            ui.horizontal(|ui| {
                ui.label("Group by");
                pick_one(
                    ui,
                    ("series_group", at),
                    &mut row.group_by,
                    &[
                        GroupBy::None,
                        GroupBy::Account,
                        GroupBy::AccountType,
                        GroupBy::Tag,
                    ],
                    group_name,
                );
                if row.group_by == GroupBy::Account {
                    ui.label("Depth");
                    ui.add(egui::TextEdit::singleline(&mut row.depth).desired_width(40.0));
                }
                if row.group_by != GroupBy::None {
                    ui.label("Top");
                    ui.add(egui::TextEdit::singleline(&mut row.top).desired_width(40.0));
                }
            });
        }
        Measure::Budget => {
            ui.horizontal(|ui| {
                ui.label("Budget");
                let chosen = budget_text(budgets, row.budget.as_deref());
                egui::ComboBox::from_id_salt(("series_budget", at))
                    .selected_text(chosen)
                    .show_ui(ui, |ui| {
                        for b in budgets {
                            let on = row.budget.as_deref().is_some_and(|f| is_budget(b, f));
                            if ui.selectable_label(on, &b.name).clicked() {
                                row.budget = Some(b.id.clone());
                            }
                        }
                    });
                ui.label("Line");
                pick_one(
                    ui,
                    ("series_line", at),
                    &mut row.line,
                    &[BudgetLine::Pace, BudgetLine::Remaining],
                    line_name,
                );
            });
        }
    }
    ui.horizontal(|ui| {
        if row.measure == Measure::Flow
            || (row.measure == Measure::Budget && row.line == BudgetLine::Pace)
        {
            ui.checkbox(&mut row.cumulative, "Cumulative");
        }
        ui.checkbox(&mut row.negate, "Negate");
        ui.label("Mark");
        pick_one(
            ui,
            ("series_mark", at),
            &mut row.mark,
            &[Mark::Line, Mark::Bar],
            mark_name,
        );
        ui.label("Label");
        ui.text_edit_singleline(&mut row.label);
    });
    action
}

// ── plots ───────────────────────────────────────────────────────────────────

const PALETTE_LIGHT: [Color32; 8] = [
    Color32::from_rgb(0x2a, 0x78, 0xd6),
    Color32::from_rgb(0xeb, 0x68, 0x34),
    Color32::from_rgb(0x1b, 0xaf, 0x7a),
    Color32::from_rgb(0xed, 0xa1, 0x00),
    Color32::from_rgb(0xe8, 0x7b, 0xa4),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x62, 0x50, 0xd6),
    Color32::from_rgb(0xe3, 0x49, 0x48),
];
const PALETTE_DARK: [Color32; 8] = [
    Color32::from_rgb(0x39, 0x87, 0xe5),
    Color32::from_rgb(0xd9, 0x59, 0x26),
    Color32::from_rgb(0x19, 0x9e, 0x70),
    Color32::from_rgb(0xc9, 0x85, 0x00),
    Color32::from_rgb(0xd5, 0x51, 0x81),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x90, 0x85, 0xe9),
    Color32::from_rgb(0xe6, 0x67, 0x67),
];
/// A plot's ninth series and later: past the categorical palette.
const OVERFLOW: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x85);

/// The dataviz reference palette, in its fixed order; validated in both modes.
fn series_color(i: usize, dark: bool) -> Color32 {
    let palette = if dark { &PALETTE_DARK } else { &PALETTE_LIGHT };
    palette.get(i).copied().unwrap_or(OVERFLOW)
}

fn number(raw: &str) -> f64 {
    form::parse_amount(raw)
        .and_then(|d| d.to_f64())
        .unwrap_or_default()
}

/// The last value a series has: what its legend shows.
fn latest(values: &[Option<String>]) -> Option<&str> {
    values.iter().rev().find_map(|v| v.as_deref())
}

fn bucket_label(interval: Interval, period: Period, date: NaiveDate) -> String {
    match interval {
        Interval::Day if period == Period::Month => date.day().to_string(),
        Interval::Day | Interval::Week => date.format("%m-%d").to_string(),
        Interval::Month => date.format("%Y-%m").to_string(),
        Interval::Year => date.format("%Y").to_string(),
    }
}

fn period_label(run: &PanelRun) -> String {
    match run.period {
        Period::Month => run.from.format("%Y-%m").to_string(),
        Period::Year => run.from.format("%Y").to_string(),
        Period::Last12Months => {
            format!("{} – {}", run.from.format("%Y-%m"), run.to.format("%Y-%m"))
        }
    }
}

/// The panel's plots stacked, sharing the x-axis: bucket `i` at x = i.
fn draw_run(ui: &mut egui::Ui, run: &PanelRun) {
    let labels: Vec<String> = run
        .buckets
        .iter()
        .map(|b| bucket_label(run.interval, run.period, *b))
        .collect();
    let n = run.plots.len().max(1);
    let dark = ui.visuals().dark_mode;
    // Plot state is kept per panel: another panel's plots start fresh.
    let key = run.panel.as_deref().unwrap_or(&run.name);
    for (i, plot) in run.plots.iter().enumerate() {
        if let Some(title) = &plot.title {
            ui.strong(title);
        }
        let left = n - i;
        let height = ((ui.available_height() - 4.0) / left as f32).max(140.0);
        draw_plot(ui, (key, i), plot, &labels, height, dark);
    }
}

fn draw_plot(
    ui: &mut egui::Ui,
    id: (&str, usize),
    plot: &PlotRun,
    labels: &[String],
    height: f32,
    dark: bool,
) {
    let count = labels.len();
    let axis_labels = labels.to_vec();
    let hover_labels = labels.to_vec();
    let has_bars = plot.series.iter().any(|s| s.mark == Mark::Bar);
    let mut p = Plot::new(("panel_plot", id))
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_x(-0.5)
        .include_x(count as f64 - 0.5)
        .height(height)
        .y_axis_min_width(72.0)
        .link_axis(egui::Id::unique("panel_x"), [true, false])
        .link_cursor(egui::Id::unique("panel_x"), [true, false])
        .x_axis_formatter(move |mark, _| {
            let x = mark.value;
            if (x - x.round()).abs() > 1e-6 || x < 0.0 {
                return String::new();
            }
            axis_labels
                .get(x.round() as usize)
                .cloned()
                .unwrap_or_default()
        })
        .y_axis_formatter(|mark, _| grouped(&format!("{}", mark.value.round() as i64)))
        .label_formatter(move |pos: &HoverPosition<'_>| match pos {
            HoverPosition::NearDataPoint {
                plot_name,
                position,
                ..
            } => Some(format!(
                "{plot_name}\n{}: {}",
                hover_labels
                    .get(position.x.round().max(0.0) as usize)
                    .cloned()
                    .unwrap_or_default(),
                grouped(&format!("{:.2}", position.y)),
            )),
            HoverPosition::Elsewhere { .. } => None,
        });
    // Bars read from zero; lines (a balance) are scaled to their data.
    if has_bars {
        p = p.include_y(0.0);
    }
    p.show(ui, |pu| {
        let mut bars: Vec<BarChart> = vec![];
        for (j, s) in plot.series.iter().enumerate() {
            let color = series_color(j, dark);
            let name = format!(
                "{}  {}",
                s.label,
                latest(&s.values).map_or("–".to_owned(), grouped)
            );
            match s.mark {
                Mark::Line => {
                    let points: Vec<[f64; 2]> = s
                        .values
                        .iter()
                        .enumerate()
                        .filter_map(|(x, v)| v.as_deref().map(|v| [x as f64, number(v)]))
                        .collect();
                    let mut line = Line::new(name, PlotPoints::from(points))
                        .color(color)
                        .width(2.0);
                    if s.dashed {
                        line = line.style(LineStyle::dashed_loose());
                    }
                    pu.line(line);
                }
                Mark::Bar => {
                    // Every bucket gets a bar, a null one at zero, so stacking lines up.
                    let items = s
                        .values
                        .iter()
                        .enumerate()
                        .map(|(x, v)| {
                            Bar::new(x as f64, v.as_deref().map_or(0.0, number)).width(0.7)
                        })
                        .collect();
                    let mut chart = BarChart::new(name, items).color(color);
                    if plot.stacked {
                        let below: Vec<&BarChart> = bars.iter().collect();
                        chart = chart.stack_on(&below);
                    }
                    bars.push(chart);
                }
            }
        }
        for chart in bars {
            pu.bar_chart(chart);
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> chrono::NaiveDate {
        s.parse().unwrap()
    }

    fn run(period: Period, interval: Interval, from: &str, to: &str) -> PanelRun {
        PanelRun {
            panel: Some("p".into()),
            name: "n".into(),
            period,
            from: d(from),
            to: d(to),
            interval,
            currency: "JPY".into(),
            buckets: vec![d(from)],
            plots: vec![],
        }
    }

    #[test]
    fn the_latest_value_is_the_last_one_there() {
        let v = vec![Some("1".to_string()), Some("2".to_string()), None];
        assert_eq!(latest(&v), Some("2"));
        assert_eq!(latest(&[None]), None);
    }

    #[test]
    fn buckets_and_periods_are_labelled_for_their_scale() {
        assert_eq!(
            bucket_label(Interval::Day, Period::Month, d("2026-10-07")),
            "7"
        );
        assert_eq!(
            bucket_label(Interval::Day, Period::Year, d("2026-10-07")),
            "10-07"
        );
        assert_eq!(
            bucket_label(Interval::Week, Period::Month, d("2026-10-05")),
            "10-05"
        );
        assert_eq!(
            bucket_label(Interval::Month, Period::Last12Months, d("2026-10-01")),
            "2026-10"
        );
        assert_eq!(
            bucket_label(Interval::Year, Period::Year, d("2026-01-01")),
            "2026"
        );
        assert_eq!(
            period_label(&run(
                Period::Month,
                Interval::Day,
                "2026-10-01",
                "2026-10-31"
            )),
            "2026-10"
        );
        assert_eq!(
            period_label(&run(
                Period::Year,
                Interval::Month,
                "2026-01-01",
                "2026-12-31"
            )),
            "2026"
        );
        assert_eq!(
            period_label(&run(
                Period::Last12Months,
                Interval::Month,
                "2025-11-01",
                "2026-10-31"
            )),
            "2025-11 – 2026-10"
        );
    }

    #[test]
    fn colours_follow_the_order_and_go_grey_past_eight() {
        assert_eq!(series_color(0, false), Color32::from_rgb(0x2a, 0x78, 0xd6));
        assert_eq!(series_color(1, true), Color32::from_rgb(0xd9, 0x59, 0x26));
        assert_eq!(series_color(7, false), Color32::from_rgb(0xe3, 0x49, 0x48));
        assert_eq!(series_color(8, true), Color32::from_rgb(0x8a, 0x8a, 0x85));
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(grouped("-1234567.5"), "-1,234,567.5");
        assert_eq!(grouped("999"), "999");
    }

    fn view() -> (tokio::runtime::Runtime, ChartsView) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let endpoint = sapphire_framework::ipc::Endpoint::in_dir(
            "test",
            std::env::temp_dir().join("charts-test"),
        );
        let client = Arc::new(LedgerClient::new("test", "0.0.0", endpoint));
        let view = ChartsView::new(runtime.handle().clone(), client, Arc::new(|| {}));
        (runtime, view)
    }

    fn panel(id: &str, name: &str) -> Panel {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-17T00:00:00+09:00").unwrap();
        Panel {
            id: id.into(),
            name: name.into(),
            period: Period::Month,
            interval: None,
            currency: None,
            created_at: at,
            updated_at: at,
            plots: vec![],
        }
    }

    #[test]
    fn a_load_overtaken_by_a_panel_change_is_dropped() {
        let (_rt, mut view) = view();
        let in_flight = view.generation.next();
        view.selected = Some("other".into());
        view.reload_now();
        let loaded = Loaded {
            panels: vec![panel("p1", "A")],
            budgets: vec![],
            accounts: vec![],
            shown: Some("p1".into()),
            run: None,
        };
        view.replies
            .0
            .send(Reply::Loaded(in_flight, Ok(Box::new(loaded))))
            .unwrap();
        view.drain_replies();
        assert_eq!(view.selected.as_deref(), Some("other"));
    }

    #[test]
    fn a_panel_named_after_a_write_is_picked() {
        let panels = vec![panel("p1", "A"), panel("p2", "生活費 pace")];
        assert_eq!(
            pick(&panels, None, Some("生活費 pace")),
            Some("p2".to_string())
        );
        assert_eq!(pick(&panels, Some("p1"), None), Some("p1".to_string()));
        assert_eq!(pick(&panels, Some("gone"), None), Some("p1".to_string()));
        let mut unsorted = vec![panel("p9", "Zed"), panel("p1", "Alpha")];
        sort_by_name(&mut unsorted);
        assert_eq!(pick(&unsorted, None, None), Some("p1".to_string()));
        assert_eq!(pick(&[], None, None), None);
    }

    #[test]
    fn a_saved_or_deleted_panel_is_followed() {
        let (_rt, mut view) = view();
        view.selected = Some("p1".into());
        view.replies
            .0
            .send(Reply::Written(
                Source::Panel,
                Some("New".into()),
                Ok("created panel".into()),
            ))
            .unwrap();
        view.drain_replies();
        assert_eq!(view.select_name.as_deref(), Some("New"));
        view.replies
            .0
            .send(Reply::Deleted(Ok("deleted panel p1".into())))
            .unwrap();
        view.drain_replies();
        assert_eq!(view.selected, None);
    }

    #[test]
    fn a_preview_hides_the_no_panels_placeholder() {
        assert!(shows_placeholder(true, false, false));
        assert!(!shows_placeholder(true, false, true));
        assert!(!shows_placeholder(false, false, false));
    }

    #[test]
    fn a_deleted_panel_ends_the_preview_and_closes_its_editor() {
        let (_rt, mut view) = view();
        view.confirm_delete = Some(panel("p1", "A"));
        view.editor = Some(PanelForm::from_definition(
            Some("p1".into()),
            &panel("p1", "A").definition(),
        ));
        view.previewing = true;
        view.replies
            .0
            .send(Reply::Deleted(Ok("deleted panel p1".into())))
            .unwrap();
        view.drain_replies();
        assert!(!view.previewing);
        assert!(view.editor.is_none());
    }

    #[test]
    fn a_write_closes_only_its_own_form() {
        let (_rt, mut view) = view();
        let send = |view: &ChartsView, source| {
            view.replies
                .0
                .send(Reply::Written(source, None, Ok("ok".into())))
                .unwrap();
        };
        let open = |view: &mut ChartsView| {
            view.budget_form = Some(BudgetForm::new("JPY"));
            view.editor = Some(PanelForm::new());
            view.previewing = true;
        };
        open(&mut view);
        send(&view, Source::Budget);
        view.drain_replies();
        assert!(view.budget_form.is_none());
        assert!(view.editor.is_some() && view.previewing);

        open(&mut view);
        send(&view, Source::Panel);
        view.drain_replies();
        assert!(view.budget_form.is_some());
        assert!(view.editor.is_none() && !view.previewing);
    }

    #[test]
    fn a_write_that_selects_a_panel_returns_to_the_current_period() {
        let (_rt, mut view) = view();
        view.offset = -3;
        view.replies
            .0
            .send(Reply::Written(Source::Panel, None, Ok("ok".into())))
            .unwrap();
        view.drain_replies();
        assert_eq!(view.offset, -3);
        view.replies
            .0
            .send(Reply::Written(
                Source::Panel,
                Some("New".into()),
                Ok("ok".into()),
            ))
            .unwrap();
        view.drain_replies();
        assert_eq!(view.offset, 0);
    }

    #[test]
    fn a_delete_is_in_flight_until_it_is_answered() {
        let (_rt, mut view) = view();
        view.deleting = true;
        view.replies
            .0
            .send(Reply::Deleted(Err("nope".into())))
            .unwrap();
        view.drain_replies();
        assert!(!view.deleting);
        view.deleting = true;
        view.replies
            .0
            .send(Reply::Deleted(Ok("deleted".into())))
            .unwrap();
        view.drain_replies();
        assert!(!view.deleting);
    }

    fn account(id: &str, name: &str) -> Account {
        Account {
            id: id.into(),
            name: name.into(),
            account_type: AccountType::Asset,
            currencies: vec![],
            opened_at: d("2026-01-01"),
            closed_at: None,
            default_tags: vec![],
            description: None,
        }
    }

    fn budget(id: &str, name: &str) -> Budget {
        Budget {
            id: id.into(),
            name: name.into(),
            tag: "t".into(),
            currency: "JPY".into(),
            monthly_amount: "1".into(),
            balance_accounts: vec![],
        }
    }

    #[test]
    fn an_account_filter_stored_as_an_id_is_listed_with_its_name() {
        let accounts = [account("4h6j2k9", "Assets:Bank")];
        let choices = form::account_choices(&accounts);
        let entries = vec!["Assets".to_string(), "4h6j2k9".into(), "gone".into()];
        let unlisted = unlisted_entries(&entries, &choices);
        assert_eq!(unlisted, ["4h6j2k9", "gone"]);
        assert_eq!(
            entry_label("4h6j2k9", &accounts),
            "Assets:Bank (id 4h6j2k9)"
        );
        assert_eq!(entry_label("gone", &accounts), "gone");
    }

    #[test]
    fn the_budget_combo_matches_an_id_or_a_name() {
        let budgets = [budget("b1", "living"), budget("b2", "play")];
        assert_eq!(budget_text(&budgets, Some("b2")), "play");
        assert_eq!(budget_text(&budgets, Some("living")), "living");
        assert_eq!(budget_text(&budgets, Some("nope")), "choose");
        assert_eq!(budget_text(&budgets, None), "choose");
    }
}
