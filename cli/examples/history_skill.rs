//! Emit generated skill artifacts without an index, models, or network.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = std::path::PathBuf::from(directory);
    let (skill, stamp) = mash::generate_history_skill()?;
    if let Some(baseline) = std::env::args_os().nth(2) {
        mash::validate_history_release(&std::fs::read_to_string(baseline)?, &stamp)?;
    }
    std::fs::create_dir_all(&directory)?;
    std::fs::write(directory.join("SKILL.md"), skill)?;
    std::fs::write(directory.join("contract.json"), stamp)?;
    Ok(())
}
