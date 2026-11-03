fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(dp_cli::run(std::env::args_os()))
}
