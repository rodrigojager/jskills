fn main() {
    match jtasks::cli::execute() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if std::env::args().any(|arg| arg == "--json") {
                println!(
                    "{}",
                    serde_json::json!({"contract_version":jtasks::read::projections::CONTRACT_VERSION,"error":{"code":"read_or_command_failed","message":format!("{error:#}")}})
                );
            } else {
                eprintln!("{error:#}");
            }
            std::process::exit(1);
        }
    }
}
