//! Deterministic child-process fixture for lifecycle tests.
//!
//! Invoked through `CARGO_BIN_EXE_eggbench-child-fixture` with one mode:
//! `exit`, `sleep`, `exit-after`, `emit-stdout`, `emit-stderr`, `emit-both`,
//! `term-exit`, `term-ignore`, or `descendant`. All modes are deterministic
//! and argv-driven; the fixture never reads secret values.

use std::io::Write;
use std::time::Duration;

fn usage() -> ! {
    eprintln!(
        "usage: eggbench-child-fixture <exit|sleep|exit-after|emit-stdout|emit-stderr|emit-both|emit-sleep|term-exit|term-ignore|descendant> [args]"
    );
    std::process::exit(2);
}

fn arg(args: &[String], index: usize) -> &str {
    args.get(index).map_or_else(|| usage(), String::as_str)
}

fn parse_u64(value: &str) -> u64 {
    value.parse::<u64>().unwrap_or_else(|_| usage())
}

fn parse_i32(value: &str) -> i32 {
    value.parse::<i32>().unwrap_or_else(|_| usage())
}

fn emit(stream: &str, total: u64) {
    let byte = if stream == "stderr" { b'e' } else { b'o' };
    let chunk = vec![byte; 8 * 1024];
    let mut remaining = total;
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    while remaining > 0 {
        let take = usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(usize::MAX);
        if stream == "stderr" {
            stderr
                .lock()
                .write_all(&chunk[..take])
                .unwrap_or_else(|_| usage());
        } else {
            stdout
                .lock()
                .write_all(&chunk[..take])
                .unwrap_or_else(|_| usage());
        }
        remaining -= take as u64;
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match arg(&args, 1) {
        "exit" => {
            std::process::exit(parse_i32(arg(&args, 2)));
        }
        "sleep" => {
            tokio::time::sleep(Duration::from_millis(parse_u64(arg(&args, 2)))).await;
        }
        "exit-after" => {
            let delay = parse_u64(arg(&args, 2));
            let code = parse_i32(arg(&args, 3));
            tokio::time::sleep(Duration::from_millis(delay)).await;
            std::process::exit(code);
        }
        "emit-stdout" => {
            emit("stdout", parse_u64(arg(&args, 2)));
        }
        "emit-stderr" => {
            emit("stderr", parse_u64(arg(&args, 2)));
        }
        "emit-both" => {
            let total = parse_u64(arg(&args, 2));
            emit("stdout", total);
            emit("stderr", total);
        }
        "emit-sleep" => {
            let total = parse_u64(arg(&args, 2));
            let delay = parse_u64(arg(&args, 3));
            emit("stdout", total);
            emit("stderr", total);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        "term-exit" => {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap_or_else(|_| usage());
                tokio::select! {
                    _ = term.recv() => std::process::exit(0),
                    () = tokio::time::sleep(Duration::from_secs(30)) => {}
                }
            }
            #[cfg(not(unix))]
            {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
        "term-ignore" => {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap_or_else(|_| usage());
                let deadline = tokio::time::sleep(Duration::from_secs(30));
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        _ = term.recv() => {}
                        () = &mut deadline => break,
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
        "descendant" => {
            let pidfile = arg(&args, 2).to_owned();
            let delay = parse_u64(arg(&args, 3));
            let exe = std::env::current_exe().unwrap_or_else(|_| usage());
            let mut child = tokio::process::Command::new(exe)
                .arg("sleep")
                .arg(delay.to_string())
                .spawn()
                .unwrap_or_else(|_| usage());
            let grandchild = child.id().unwrap_or_else(|| usage());
            std::fs::write(&pidfile, grandchild.to_string()).unwrap_or_else(|_| usage());
            let _ = child.wait().await;
        }
        _ => usage(),
    }
}
