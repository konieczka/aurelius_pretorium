//! The E2E tests' backend (see e2e/README.md): `e2e_harness [address]`.
fn main() {
    fleet_command_lib::harness::serve(&std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:7811".into()));
}
