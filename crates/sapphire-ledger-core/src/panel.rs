//! A panel: a saved chart — its period and, per plot, the series queries to
//! draw. A new chart is a new file, not a new method; an agent runs the same
//! files as query presets.

use chrono::{DateTime, Datelike, FixedOffset, Months, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::repository::Workspace;
use crate::series::{Interval, Measure, SeriesQuery, SeriesRequest, check_query, query_series};

/// The range a panel shows, relative to an anchor date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    #[serde(rename = "month")]
    Month,
    #[serde(rename = "year")]
    Year,
    #[serde(rename = "last_12_months")]
    Last12Months,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Line,
    Bar,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A series as a panel keeps it: the query, and how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSeries {
    #[serde(flatten)]
    pub query: SeriesQuery,
    /// Default line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
}

/// One plot: its own y-axis, the x-axis shared with the panel's other plots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Stack the plot's bars.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stacked: bool,
    pub series: Vec<PanelSeries>,
}

/// What a panel is, without its identity: what `create_panel` and
/// `update_panel` take, and what `run_panel` runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelDefinition {
    pub name: String,
    pub period: Period,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub plots: Vec<Plot>,
}

/// `panels/{id}.toml`. Plots last, so TOML writes `[[plots]]` after the
/// plain keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub name: String,
    pub period: Period,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub plots: Vec<Plot>,
}

impl Panel {
    pub fn definition(&self) -> PanelDefinition {
        PanelDefinition {
            name: self.name.clone(),
            period: self.period,
            interval: self.interval,
            currency: self.currency.clone(),
            plots: self.plots.clone(),
        }
    }
}

pub fn default_interval(period: Period) -> Interval {
    match period {
        Period::Month => Interval::Day,
        Period::Year | Period::Last12Months => Interval::Month,
    }
}

fn shift_months(first: NaiveDate, by: i32) -> Option<NaiveDate> {
    if by >= 0 {
        first.checked_add_months(Months::new(by as u32))
    } else {
        first.checked_sub_months(Months::new(by.unsigned_abs()))
    }
}

fn month_end(first: NaiveDate) -> Option<NaiveDate> {
    first.checked_add_months(Months::new(1))?.pred_opt()
}

/// The dates `period` covers around `anchor`, moved by `offset` periods
/// (a month for `last_12_months`).
pub fn panel_range(
    period: Period,
    anchor: NaiveDate,
    offset: i32,
) -> Result<(NaiveDate, NaiveDate)> {
    let out_of_range = || Error::Validation(format!("offset {offset} is out of range"));
    let this_month =
        NaiveDate::from_ymd_opt(anchor.year(), anchor.month(), 1).expect("a date's month exists");
    match period {
        Period::Month => {
            let first = shift_months(this_month, offset).ok_or_else(out_of_range)?;
            Ok((first, month_end(first).ok_or_else(out_of_range)?))
        }
        Period::Year => {
            let year = anchor.year().checked_add(offset).ok_or_else(out_of_range)?;
            let first = NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(out_of_range)?;
            let last = NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(out_of_range)?;
            Ok((first, last))
        }
        Period::Last12Months => {
            let end = shift_months(this_month, offset).ok_or_else(out_of_range)?;
            let first = shift_months(end, -11).ok_or_else(out_of_range)?;
            Ok((first, month_end(end).ok_or_else(out_of_range)?))
        }
    }
}

fn message(e: Error) -> String {
    match e {
        Error::Validation(m) => m,
        other => other.to_string(),
    }
}

/// Refuse a definition that cannot be drawn: no name, no plots, a plot
/// without series, or a series whose fields do not fit its measure.
pub fn check_panel(def: &PanelDefinition) -> Result<()> {
    let name = def.name.trim();
    if name.is_empty() {
        return Err(Error::Validation("a panel needs a name".into()));
    }
    if def.plots.is_empty() {
        return Err(Error::Validation(format!("panel {name} has no plots")));
    }
    for (i, plot) in def.plots.iter().enumerate() {
        if plot.series.is_empty() {
            return Err(Error::Validation(format!(
                "panel {name}: plot {} has no series",
                i + 1
            )));
        }
        for s in &plot.series {
            check_query(&s.query).map_err(|e| {
                Error::Validation(format!("panel {name}: plot {}: {}", i + 1, message(e)))
            })?;
        }
    }
    Ok(())
}

/// A panel by id, else by name.
pub fn find_panel<'a>(panels: &'a [Panel], needle: &str) -> Result<&'a Panel> {
    panels
        .iter()
        .find(|p| p.id == needle)
        .or_else(|| panels.iter().find(|p| p.name == needle))
        .ok_or_else(|| Error::Validation(format!("no panel {needle}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesRun {
    pub label: String,
    pub group: Option<String>,
    pub mark: Mark,
    /// A planned line (a budget's), drawn dashed.
    pub dashed: bool,
    pub values: Vec<Option<Decimal>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotRun {
    pub title: Option<String>,
    pub stacked: bool,
    pub series: Vec<SeriesRun>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRun {
    pub name: String,
    pub period: Period,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    pub currency: String,
    pub buckets: Vec<NaiveDate>,
    pub plots: Vec<PlotRun>,
}

/// Run a panel's queries as one query, each result placed in its plot.
pub fn run_panel(
    ws: &Workspace,
    def: &PanelDefinition,
    anchor: NaiveDate,
    offset: i32,
    today: NaiveDate,
) -> Result<PanelRun> {
    check_panel(def)?;
    let (from, to) = panel_range(def.period, anchor, offset)?;
    let interval = def.interval.unwrap_or_else(|| default_interval(def.period));
    let sources: Vec<(usize, &PanelSeries)> = def
        .plots
        .iter()
        .enumerate()
        .flat_map(|(i, plot)| plot.series.iter().map(move |s| (i, s)))
        .collect();
    let result = query_series(
        ws,
        &SeriesRequest {
            from,
            to,
            interval,
            currency: def.currency.clone(),
            series: sources.iter().map(|(_, s)| s.query.clone()).collect(),
        },
        today,
    )?;
    let mut plots: Vec<PlotRun> = def
        .plots
        .iter()
        .map(|p| PlotRun {
            title: p.title.clone(),
            stacked: p.stacked,
            series: vec![],
        })
        .collect();
    for data in result.series {
        let (plot, source) = sources[data.index];
        plots[plot].series.push(SeriesRun {
            label: data.label,
            group: data.group,
            mark: source.mark.unwrap_or(Mark::Line),
            dashed: source.query.measure == Measure::Budget,
            values: data.values,
        });
    }
    Ok(PanelRun {
        name: def.name.clone(),
        period: def.period,
        from,
        to,
        interval,
        currency: result.currency,
        buckets: result.buckets,
        plots,
    })
}
