use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                let Some(p) = args.next() else {
                    eprintln!("ferncheck: --root needs a path");
                    return ExitCode::from(2);
                };
                root = PathBuf::from(p);
            }
            "-h" | "--help" => {
                println!("usage: ferncheck [--root <repo>]");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("ferncheck: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }

    match ferncheck::run_all(&root) {
        Ok(violations) if violations.is_empty() => {
            println!("ferncheck: ok");
            ExitCode::SUCCESS
        }
        Ok(violations) => {
            for v in &violations {
                eprintln!("{v}");
            }
            eprintln!("ferncheck: {} problem(s)", violations.len());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("ferncheck: {e}");
            ExitCode::from(2)
        }
    }
}
