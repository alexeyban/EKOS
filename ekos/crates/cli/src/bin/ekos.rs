//! The public `ekos` binary: the whole CLI with no out-of-tree extensions (RFC 0149).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ekos::app::main_with(ekos::extension::Extensions::none()).await
}
