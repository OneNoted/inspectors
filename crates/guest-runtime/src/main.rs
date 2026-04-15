use guest_runtime::RuntimeConfig;

#[tokio::main]
async fn main() {
    guest_runtime::run(RuntimeConfig::from_env_and_args()).await;
}
