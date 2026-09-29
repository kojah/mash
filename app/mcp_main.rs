#[tokio::main]
async fn main() {
    std::process::exit(mash::mcp_entry().await);
}
