//! Generate the draft public contract from its owning Rust models.
use clap::{Parser, ValueEnum};
use mash::connectors::{Batch, Receipt};
use rmcp::schemars;

#[derive(Parser)]
struct Arguments {
    kind: Kind,
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Batch,
    Receipt,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = match Arguments::parse().kind {
        Kind::Batch => schemars::schema_for!(Batch),
        Kind::Receipt => schemars::schema_for!(Receipt),
    };
    serde_json::to_writer_pretty(std::io::stdout().lock(), &schema)?;
    Ok(())
}
