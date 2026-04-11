use guest_runtime::RuntimeConfig;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let should_run_guest_runtime = args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--host" | "--port" | "--artifacts-dir" | "--browser-command"
        )
    }) && !args
        .iter()
        .any(|arg| arg == "--activate-desktop" || arg == "--session");

    if should_run_guest_runtime {
        let runtime = tokio::runtime::Runtime::new().expect("guest runtime tokio runtime");
        runtime.block_on(guest_runtime::run(RuntimeConfig::from_env_and_args()));
        return;
    }

    desktop_app::run();
}
