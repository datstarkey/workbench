//! CLI vs gix timings for each switched read, on this repository.

use std::path::Path;

use crate::git;
use crate::git_read;

/// `cargo test -p workbench-core --release -- --ignored --nocapture bench_gix_vs_cli`
#[test]
#[ignore]
fn bench_gix_vs_cli() {
    use std::time::Instant;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = repo.to_str().unwrap();
    const N: u32 = 50;
    fn time<T>(n: u32, f: impl Fn() -> T) -> f64 {
        f();
        let start = Instant::now();
        for _ in 0..n {
            f();
        }
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(n)
    }
    let rows: [(&str, f64, f64); 6] = [
        (
            "status branch",
            time(N, || git::status_branch_cli(path)),
            time(N, || git_read::status_branch(path).unwrap().unwrap()),
        ),
        (
            "git_info",
            time(N, || git::git_info_cli(path).unwrap()),
            time(N, || git_read::git_info(path).unwrap().unwrap()),
        ),
        (
            "list_worktrees",
            time(N, || git::list_worktrees_cli(path).unwrap()),
            time(N, || git_read::list_worktrees(path).unwrap().unwrap()),
        ),
        (
            "list_branches",
            time(N, || git::list_branches_cli(path).unwrap()),
            time(N, || git_read::list_branches(path).unwrap()),
        ),
        (
            "git_log(50)",
            time(N, || git::git_log_cli(path, 50).unwrap()),
            time(N, || git_read::git_log(path, 50).unwrap().unwrap()),
        ),
        (
            "git_stash_list",
            time(N, || git::git_stash_list_cli(path).unwrap()),
            time(N, || git_read::git_stash_list(path).unwrap().unwrap()),
        ),
    ];
    for (name, cli, fast) in rows {
        println!(
            "{name:16} cli {cli:8.3} ms   gix {fast:8.3} ms   x{:.1}",
            cli / fast
        );
    }
}
