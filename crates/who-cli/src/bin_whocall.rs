#[allow(dead_code)]
mod cmd_callers;
mod output;

use std::process;

use clap::Parser;

use who_core::error::ExitCode;

#[derive(Parser)]
#[command(name = "whocall", version, about = "Find semantic callers of a symbol")]
struct Cli {
    /// Target (file:line, file:line:col, file#symbol, qualified::symbol, or plain name)
    target: String,

    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// Output as NDJSON
    #[arg(long)]
    ndjson: bool,

    /// Output in quickfix format
    #[arg(long, value_name = "FORMAT")]
    format: Option<String>,

    /// Show explain/why information
    #[arg(long)]
    why: bool,
}

fn main() {
    let cli = Cli::parse();

    let output_opts = output::OutputOpts {
        json: cli.json,
        ndjson: cli.ndjson,
        format: cli.format,
        why: cli.why,
    };

    let result = cmd_callers::run(&cli.target, &output_opts);

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        let code = e
            .downcast_ref::<who_core::error::WhoError>()
            .map(|e| e.exit_code().code())
            .unwrap_or(ExitCode::InternalError.code());
        process::exit(code);
    }
}
