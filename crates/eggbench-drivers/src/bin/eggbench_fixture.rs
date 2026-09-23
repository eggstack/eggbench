//! Deterministic test fixture executable for driver-substrate tests.
//!
//! Invoked through `argv` only (no shell). Behavior is selected by argv:
//!
//! - `version` prints `eggbench-fixture 1.2.3` to stdout, exit 0;
//! - `stdout-bytes N` prints N `x` bytes to stdout;
//! - `stderr-bytes N` prints N `e` bytes to stderr;
//! - `both-bytes N M` prints N stdout + M stderr bytes concurrently;
//! - `exit N` exits with code N;
//! - `sleep MS` sleeps MS milliseconds (cancellation target);
//! - `malformed` prints non-JSON `not-json{{{` to stdout;
//! - `spawn-child` spawns a 30s sleep child on Unix then sleeps (cleanup test).
//!
//! Not published as a user-facing driver.

use std::io::Write;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map_or("version", String::as_str);
    match command {
        "version" => {
            println!("eggbench-fixture 1.2.3");
        }
        "stdout-bytes" => {
            let n: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            for _ in 0..n {
                handle.write_all(b"x").unwrap();
            }
        }
        "stderr-bytes" => {
            let n: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let stderr = std::io::stderr();
            let mut handle = stderr.lock();
            for _ in 0..n {
                handle.write_all(b"e").unwrap();
            }
        }
        "both-bytes" => {
            let stdout_n: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let stderr_n: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for _ in 0..stdout_n {
                out.write_all(b"x").unwrap();
            }
            out.flush().unwrap();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            for _ in 0..stderr_n {
                err.write_all(b"e").unwrap();
            }
        }
        "exit" => {
            let code: i32 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            std::process::exit(code);
        }
        "sleep" => {
            let ms: u64 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(60_000);
            std::thread::sleep(Duration::from_millis(ms));
        }
        "malformed" => {
            println!("not-json{{{{");
        }
        "spawn-child" => {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("sleep").arg("30").spawn();
            }
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => {
            eprintln!("unknown fixture command {command}");
            std::process::exit(2);
        }
    }
}
