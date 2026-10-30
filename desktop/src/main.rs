//! sapphire-ledger's desktop app: the books on the framework's shell, with
//! its Settings — and, given a subcommand, the CLI.
//!
//! This one binary is a superset of the CLI's: bare, it opens the window;
//! with `serve`, `mcp`, `tx`, `status`, … it runs the CLI's own code
//! (`sapphire_ledger_cli::run`), so the service manager and MCP clients can
//! start it as they start `sapphire-ledger`.
//!
//! The window is a pure client, like sapphire-sync's: it talks to the
//! service-managed `sapphire-ledger serve` over IPC, which relays what the
//! bridge has to say, and never starts either itself. The Ledger screen calls
//! the server's `sapphire-ledger.*` methods, so it shows the workspace the
//! server has selected; the Charts screen draws the workspace's panels from
//! `run_panel`; Settings, at the rail's foot, picks that workspace and manages
//! the workgroup. Closing the window stops nothing.

mod charts;
mod client;
mod form;
mod ledger;

use std::sync::Arc;

use clap::Parser;
use sapphire_framework::gui::app::{AppOptions, AppSetup};
use sapphire_framework::gui::client::AppIdentity;
use sapphire_framework::gui::{Destination, Settings, Shell, ShellApp, Side, eframe, egui};
use sapphire_ledger_cli::Edition;
use sapphire_ledger_cli::cli::{Command, GlobalArgs};

use crate::charts::ChartsView;
use crate::client::LedgerClient;
use crate::ledger::LedgerView;

const APP: AppIdentity = AppIdentity {
    app_name: "sapphire-ledger",
    version: env!("CARGO_PKG_VERSION"),
};

/// The app's own screens, on the framework's rail.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Ledger,
    Charts,
}

struct App {
    shell: Shell<Screen>,
    views: Views,
}

/// The screens' state, which the shell draws as a [`ShellApp`].
struct Views {
    ledger: LedgerView,
    charts: ChartsView,
}

impl ShellApp<Screen> for Views {
    fn screen_ui(&mut self, ui: &mut egui::Ui, screen: Screen, side: &mut Side) {
        match screen {
            Screen::Ledger => self.ledger.ui(ui, side),
            Screen::Charts => {
                egui::CentralPanel::default().show(ui, |ui| self.charts.ui(ui));
            }
        }
    }

    /// The Ledger's picked transaction or account, or its form; the other
    /// screens pick nothing.
    fn side_ui(&mut self, ui: &mut egui::Ui, screen: Option<Screen>, side: &mut Side) {
        match screen {
            Some(Screen::Ledger) => self.ledger.side_ui(ui, side),
            Some(Screen::Charts) | None => {
                ui.weak("Nothing selected.");
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.show(ui, &mut self.views);
    }
}

/// The CLI's command line, with the subcommand optional: none opens the window.
#[derive(Parser)]
#[command(
    name = "sapphire-ledger",
    about = "Local-first double-entry household ledger: the desktop app, or the CLI's commands",
    version
)]
struct DesktopCli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> anyhow::Result<()> {
    let cli = DesktopCli::parse();
    match cli.command {
        Some(command) => sapphire_ledger_cli::run(command, cli.global, Edition::Desktop),
        None => Ok(window()?),
    }
}

fn window() -> eframe::Result<()> {
    sapphire_framework::gui::app::run(
        AppOptions::new(APP, "Sapphire Ledger")
            .inner_size([1000.0, 640.0])
            .min_inner_size([720.0, 440.0]),
        |setup: AppSetup<'_>| {
            let ledger_client = Arc::new(LedgerClient::new(
                APP.app_name,
                APP.version,
                setup.endpoints.app,
            ));
            let shell = Shell::new(
                vec![
                    Destination::new(
                        Screen::Ledger,
                        egui::include_image!("../assets/icons/wallet.svg"),
                        "Ledger",
                    ),
                    Destination::new(
                        Screen::Charts,
                        egui::include_image!("../assets/icons/chart-line.svg"),
                        "Charts",
                    ),
                ],
                Settings::new(setup.framework),
            );
            Box::new(App {
                shell,
                views: Views {
                    ledger: LedgerView::new(
                        setup.runtime.clone(),
                        Arc::clone(&ledger_client),
                        Arc::clone(&setup.repaint),
                    ),
                    charts: ChartsView::new(setup.runtime, ledger_client, setup.repaint),
                },
            })
        },
    )
}
