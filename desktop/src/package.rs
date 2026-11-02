//! Sapphire Ledger as a Velopack package (#62), on the framework's `gui::package`:
//! [`PACKAGE`] is what `main` hands to `package::start`, its one service is the app server's
//! `serve`, and its command goes on `PATH` as `sapphire-ledger`.

use sapphire_framework::gui::package::runtime::{CliSpec, PackageSpec, Services};
use sapphire_framework::gui::package::{AppPackage, ServiceSpec};

/// The package: its command, and the app server's service, which an update stops and starts.
pub static PACKAGE: PackageSpec = PackageSpec {
    app_name: "sapphire-ledger",
    on_installed: None,
    on_uninstalling: None,
    before_apply: None,
    cli: Some(CliSpec {
        command: "sapphire-ledger",
    }),
    services: Services::new(&[app_service]),
};

/// The package as the framework runs it: released with this repository's `v<version>`
/// releases.
pub const APP_PACKAGE: AppPackage = AppPackage {
    name: "Sapphire Ledger",
    github: ("sapphire-turtle", "sapphire-ledger"),
    spec: &PACKAGE,
    service: app_service,
};

/// The app server's service, for this binary's version.
pub fn app_service() -> ServiceSpec {
    sapphire_framework::server::app_service_spec("sapphire-ledger", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_the_app_and_its_service() {
        assert_eq!(PACKAGE.app_name, "sapphire-ledger");
        assert_eq!(APP_PACKAGE.command(), "sapphire-ledger");
        assert_eq!(APP_PACKAGE.desktop_asset(), "sapphire-ledger-desktop");
        let service = app_service();
        assert_eq!(service.app_name, "sapphire-ledger");
        assert_eq!(service.args, ["serve"]);
    }
}
