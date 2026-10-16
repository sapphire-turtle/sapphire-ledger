//! sapphire-ledger's desktop app: the books on the framework's shell, with
//! its Settings.
//!
//! A pure client, like sapphire-sync's: it talks to the service-managed
//! `sapphire-ledger serve` and `sapphire-bridge` over IPC and never starts
//! either itself. The Ledger screen calls the server's `sapphire-ledger.*`
//! methods, so it shows the workspace the server has selected; the Charts
//! screen draws a budget's month from `budget_report`; Settings, at the
//! rail's foot, picks that workspace and manages the workgroup. Closing the
//! window stops nothing.

mod charts;
mod client;
mod form;
mod ledger;

use std::sync::Arc;

use sapphire_framework::gui::app::{AppOptions, AppSetup};
use sapphire_framework::gui::client::AppIdentity;
use sapphire_framework::gui::{Destination, Settings, Shell, eframe, egui};

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
    ledger: LedgerView,
    charts: ChartsView,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let (ledger, charts) = (&mut self.ledger, &mut self.charts);
        self.shell.ui(ui, |ui, screen| match screen {
            Screen::Ledger => {
                egui::CentralPanel::default().show(ui, |ui| ledger.ui(ui));
            }
            Screen::Charts => {
                egui::CentralPanel::default().show(ui, |ui| charts.ui(ui));
            }
        });
    }
}

fn main() -> eframe::Result<()> {
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
                ledger: LedgerView::new(
                    setup.runtime.clone(),
                    Arc::clone(&ledger_client),
                    Arc::clone(&setup.repaint),
                ),
                charts: ChartsView::new(setup.runtime, ledger_client, setup.repaint),
            })
        },
    )
}
