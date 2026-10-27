fn main() -> Result<(), Box<dyn std::error::Error>> {
    ruvane::ClientPrerender::write_build_module()?;
    Ok(())
}
