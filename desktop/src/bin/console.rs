//! The console shim the Windows package puts on `PATH` as `sapphire-ledger.exe` (#62):
//! `sapphire-ledger-desktop.exe` is built for the GUI subsystem, so a command typed in a
//! terminal would print nothing. This runs it with the same arguments and streams, waits,
//! and exits with its code (corundum-framework-package's `console::forward`).

#[cfg(windows)]
fn main() -> ! {
    sapphire_framework::gui::package::runtime::console::forward("sapphire-ledger-desktop.exe")
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "sapphire-ledger-console is the Windows package's command; run sapphire-ledger-desktop"
    );
    std::process::ExitCode::FAILURE
}
