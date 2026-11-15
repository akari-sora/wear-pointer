//! Developer & terminal CLI entry point (`wear-pointer`).
//!
//! Preserves all flags (`--debug`, `--backend`, `--mode`, `--dpi`, `--trace`,
//! `--replay`, etc.). Also turns on the macOS Menu Bar UI automatically when
//! `WearPointer.app` is double-clicked in Finder.

fn main() -> anyhow::Result<()> {
    wear_pointer::run_cli_entry()
}
