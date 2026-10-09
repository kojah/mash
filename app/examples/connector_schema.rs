//! Generate the draft public contract from its owning Rust models.
use clap::{Parser, ValueEnum};
use mash::connectors::{Batch, MAX_BATCH_BYTES, MAX_CONTENT_BYTES, MAX_METADATA_BYTES, Receipt};
use rmcp::schemars;
use std::io::Read;

#[derive(Parser)]
struct Arguments {
    kind: Kind,
    /// Test-only canonical ingestion destination; never a provider's database.
    #[arg(long, required_if_eq("kind", "apply"))]
    database: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Batch,
    Receipt,
    Validate,
    Apply,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let kind = arguments.kind;
    if matches!(kind, Kind::Validate | Kind::Apply) {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take((MAX_BATCH_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let batch = Batch::decode(&bytes)?;
        if matches!(kind, Kind::Apply) {
            let path = arguments.database.ok_or("apply requires a database")?;
            let receipt = mash::connectors::silver::Store::open(&path)?.apply(&batch)?;
            serde_json::to_writer(std::io::stdout().lock(), &receipt)?;
        } else {
            println!("accepted");
        }
        return Ok(());
    }
    let schema = match kind {
        Kind::Batch => schemars::schema_for!(Batch),
        Kind::Receipt => schemars::schema_for!(Receipt),
        Kind::Validate | Kind::Apply => unreachable!(),
    };
    let mut schema = serde_json::to_value(schema)?;
    if matches!(kind, Kind::Batch) {
        schema["x-mash-limits"] = serde_json::json!({
            "batch_bytes": MAX_BATCH_BYTES,
            "content_bytes": MAX_CONTENT_BYTES,
            "metadata_bytes": MAX_METADATA_BYTES,
        });
    }
    serde_json::to_writer_pretty(std::io::stdout().lock(), &schema)?;
    Ok(())
}
