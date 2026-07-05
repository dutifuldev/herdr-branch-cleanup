use std::collections::BTreeMap;

use herdr_branch_cleanup::cli;
use herdr_branch_cleanup::daemon::SystemSpawner;
use herdr_branch_cleanup::procio::SystemRunner;

fn main() {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "status".to_owned());
    let environ: BTreeMap<String, String> = std::env::vars().collect();
    let entrypoint = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "herdr-branch-cleanup".to_owned());
    let ctx = cli::context_from(&environ, entrypoint);
    let code = cli::dispatch(&command, &ctx, &SystemRunner, &SystemSpawner, &mut |text| {
        println!("{text}");
    });
    std::process::exit(code);
}
