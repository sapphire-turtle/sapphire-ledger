//! sapphire-ledger's desktop app: the books, and the framework's `SyncPanel`.
//!
//! A pure client, like sapphire-sync's: it talks to the service-managed
//! `sapphire-ledger serve` and `sapphire-bridge` over IPC and never starts
//! either itself. The Ledger tab calls the server's `sapphire-ledger.*`
//! methods, so it shows the workspace the server has selected; the Sync tab
//! picks that workspace and manages the workgroup. Closing the window stops
//! nothing.

mod client;
mod form;
mod gpu;
mod ledger;

use std::sync::Arc;

use eframe::egui;
use sapphire_framework::gui::SyncPanel;
use sapphire_framework::gui::client::{AppIdentity, ClientConfig, FrameworkClient};

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
    sync: SyncPanel,
    // Keeps the clients' background tasks alive for the window's lifetime.
    _runtime: tokio::runtime::Runtime,
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
    gpu::init_logging();
    gpu::install_panic_hook();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    let config = ClientConfig::new(APP).expect("the runtime directory and this executable's path");
    let ledger_client = LedgerClient::new(APP.app_name, APP.version, config.endpoints.app.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([720.0, 440.0])
            .with_title("Sapphire Ledger"),
        wgpu_options: gpu::wgpu_options(),
        ..Default::default()
    };
    eframe::run_native(
        "Sapphire Ledger",
        options,
        Box::new(move |cc| {
            gpu::on_render_state(cc.wgpu_render_state.as_ref());
            // CJK glyphs from a font the OS already has: nothing is bundled.
            if sapphire_framework::gui::fonts::install_system_cjk_fallback(&cc.egui_ctx).is_none() {
                tracing::warn!("no CJK font found on this system; Japanese text will not render");
            }
            let ctx = cc.egui_ctx.clone();
            let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
            let framework = FrameworkClient::spawn(runtime.handle(), config, Arc::clone(&repaint));
            Ok(Box::new(App {
                tab: Tab::Ledger,
                ledger: LedgerView::new(runtime.handle().clone(), ledger_client, repaint),
                sync: SyncPanel::new(framework),
                _runtime: runtime,
            }))
        }),
    )
}
