fn main() -> nice_plug_xtask::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "bundle-au") {
        if !cfg!(target_os = "macos") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "AU builds require macOS; use 'bundle' for VST3/CLAP on other platforms",
            )
            .into());
        }
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/build_au.py");
        let status = std::process::Command::new("python3")
            .arg(script)
            .args(&args[1..])
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other("AU build failed; see output above").into());
        }
        return Ok(());
    }
    nice_plug_xtask::main()
}
