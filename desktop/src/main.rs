//! sapphire-ledger's desktop app: the books, and the framework's Settings.
//!
//! A pure client, like sapphire-sync's: it talks to the service-managed
//! `sapphire-ledger serve` and `sapphire-bridge` over IPC and never starts
//! either itself. The Ledger tab calls the server's `sapphire-ledger.*`
//! methods, so it shows the workspace the server has selected; the Sync tab
//! picks that workspace and manages the workgroup. Closing the window stops
//! nothing.

mod client;
mod form;
mod ledger;

use sapphire_framework::gui::app::{AppOptions, AppSetup};
use sapphire_framework::gui::client::AppIdentity;
use sapphire_framework::gui::{Settings, eframe, egui};

use crate::client::LedgerClient;
use crate::ledger::LedgerView;

const APP: AppIdentity = AppIdentity {
    app_name: "sapphire-ledger",
    version: env!("CARGO_PKG_VERSION"),
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Ledger,
    Sync,
}

struct App {
    tab: Tab,
    ledger: LedgerView,
    sync: Settings,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Ledger, "Ledger");
                ui.selectable_value(&mut self.tab, Tab::Sync, "Sync");
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Ledger => self.ledger.ui(ui),
            Tab::Sync => self.sync.ui(ui),
        });
    }
}

fn main() -> eframe::Result<()> {
    sapphire_framework::gui::app::run(
        AppOptions::new(APP, "Sapphire Ledger")
            .inner_size([1000.0, 640.0])
            .min_inner_size([720.0, 440.0]),
        |setup: AppSetup<'_>| {
            let ledger_client = LedgerClient::new(APP.app_name, APP.version, setup.endpoints.app);
            Box::new(App {
                tab: Tab::Ledger,
                ledger: LedgerView::new(setup.runtime, ledger_client, setup.repaint),
                sync: Settings::new(setup.framework),
            })
        },
    )
}
