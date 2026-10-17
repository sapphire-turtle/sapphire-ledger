//! The series query and panels on the wire: what `query_series` and the
//! panel methods take and answer. The same shapes as core's, so a panel file
//! reads as a definition here.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::AccountType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// What moved in each bucket: expenses positive, income negative.
    Flow,
    /// What was held at the end of each bucket.
    Balance,
    /// A budget's monthly amount spread over the days.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    Day,
    /// Starts on Monday.
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    None,
    Account,
    AccountType,
    Tag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLine {
    /// The pro-rated allowance.
    Pace,
    /// The budget's balance accounts at the start, minus the cumulative pace.
    Remaining,
}

/// The range a panel shows around its anchor date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Period {
    /// The calendar month containing the anchor.
    #[serde(rename = "month")]
    Month,
    /// The calendar year.
    #[serde(rename = "year")]
    Year,
    /// Twelve calendar months ending with the anchor's.
    #[serde(rename = "last_12_months")]
    Last12Months,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Line,
    Bar,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One series. Filters combine with AND across fields and OR within a list;
/// a field that does not apply to the measure is an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SeriesQuery {
    pub measure: Measure,
    /// flow/balance: account ids or names. A name matches that account and
    /// every account under it (`Expenses:Food` matches `Expenses:Food:Lunch`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
    /// flow/balance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_types: Vec<AccountType>,
    /// flow/balance: the transaction carries any of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// budget: the budget's id or name. Required for a budget series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    /// budget: `pace` (default) or `remaining`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<BudgetLine>,
    /// flow/balance: split into one series per group. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    /// With group_by = account: group by the first `depth` name segments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Grouped: keep the largest `top` groups, sum the rest into `Other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<u32>,
    /// flow and budget pace: a running total from the range's start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cumulative: bool,
    /// Flip the sign, e.g. to show income as positive.
    #[serde(default, skip_serializing_if = "is_false")]
    pub negate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl SeriesQuery {
    pub fn new(measure: Measure) -> SeriesQuery {
        SeriesQuery {
            measure,
            accounts: vec![],
            account_types: vec![],
            tags: vec![],
            budget: None,
            line: None,
            group_by: None,
            depth: None,
            top: None,
            cumulative: false,
            negate: false,
            label: None,
        }
    }
}

/// A series as a panel keeps it: the query, and how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelSeries {
    #[serde(flatten)]
    pub query: SeriesQuery,
    /// `line` (default) or `bar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
}

/// One plot: its own y-axis, the x-axis shared with the panel's others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Plot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Stack the plot's bars.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stacked: bool,
    pub series: Vec<PanelSeries>,
}

/// A panel without its identity: what is saved, and what is run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelDefinition {
    /// Unique among panels.
    pub name: String,
    pub period: Period,
    /// Default: day for a month, month otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    /// Default: the ledger's base currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub plots: Vec<Plot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuerySeriesParams {
    /// `YYYY-MM-DD`, included.
    pub from: String,
    /// `YYYY-MM-DD`, included.
    pub to: String,
    pub interval: Interval,
    /// Default: the ledger's base currency. Postings in others are ignored.
    pub currency: Option<String>,
    /// At least one. All share the same buckets.
    pub series: Vec<SeriesQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunPanelParams {
    /// A saved panel's id or name. Give this or `definition`.
    pub panel: Option<String>,
    /// An unsaved panel to run as it is. Give this or `panel`.
    pub definition: Option<PanelDefinition>,
    /// `YYYY-MM-DD` the period is around. Default today.
    pub anchor: Option<String>,
    /// Periods to move by: -1 is the previous month (or year). Default 0.
    pub offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddPanelParams {
    pub definition: PanelDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdatePanelParams {
    /// The panel's id or name.
    pub panel: String,
    /// Replaces the old definition whole.
    pub definition: PanelDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeletePanelParams {
    /// The panel's id or name.
    pub panel: String,
}

/// One result series. Amounts are decimals as strings; `null` is a bucket
/// after today in an actual (flow or balance) series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesData {
    /// The query it came from.
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub label: String,
    pub values: Vec<Option<String>>,
}

/// What `query_series` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesResult {
    pub currency: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    /// Each bucket's first date inside the range.
    pub buckets: Vec<NaiveDate>,
    pub series: Vec<SeriesData>,
}

/// One saved panel, as `list_panels` answers.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesRun {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub mark: Mark,
    /// A planned line (a budget's).
    pub dashed: bool,
    pub values: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlotRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub stacked: bool,
    pub series: Vec<SeriesRun>,
}

/// What `run_panel` answers: the panel's queries run, each series in its plot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRun {
    /// The saved panel's id; absent for a definition run as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    pub name: String,
    pub period: Period,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    pub currency: String,
    pub buckets: Vec<NaiveDate>,
    pub plots: Vec<PlotRun>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_reads_as_a_hand_written_one() {
        let json = r#"{
            "name": "Food", "period": "last_12_months",
            "plots": [{ "series": [
                { "measure": "flow", "account_types": ["Expense"], "group_by": "account_type", "mark": "bar" },
                { "measure": "budget", "budget": "b", "line": "remaining" }
            ]}]
        }"#;
        let def: PanelDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.period, Period::Last12Months);
        let s = &def.plots[0].series[0];
        assert_eq!(s.query.group_by, Some(GroupBy::AccountType));
        assert_eq!(s.mark, Some(Mark::Bar));
        assert!(!def.plots[0].stacked);
        let back = serde_json::to_value(&def).unwrap();
        assert_eq!(back["plots"][0]["series"][1]["line"], "remaining");
        // Defaults are left out, as a person would write it.
        assert!(back["plots"][0]["series"][0].get("cumulative").is_none());
        assert!(back.get("interval").is_none());
    }

    #[test]
    fn run_panel_takes_a_name_or_a_definition() {
        let p: RunPanelParams =
            serde_json::from_str(r#"{"panel":"生活費 pace","offset":-1}"#).unwrap();
        assert_eq!(p.offset, Some(-1));
        assert!(p.definition.is_none());
    }

    #[test]
    fn the_params_have_object_schemas() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(QuerySeriesParams)).unwrap(),
            serde_json::to_value(schemars::schema_for!(RunPanelParams)).unwrap(),
            serde_json::to_value(schemars::schema_for!(AddPanelParams)).unwrap(),
        ] {
            assert_eq!(schema["type"], "object", "{schema}");
        }
    }
}
