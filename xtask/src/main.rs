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
        return python_script(&script, &args[1..]);
    }
    nice_plug_xtask::main()?;
    if args
        .first()
        .is_some_and(|arg| arg == "bundle" || arg == "bundle-universal")
    {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/prepare_bundles.py");
        python_script(&script, &[])?;
    }
    Ok(())
}

fn python_script(script: &std::path::Path, args: &[String]) -> nice_plug_xtask::Result<()> {
    let python = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };
    let status = std::process::Command::new(python)
        .arg(script)
        .args(args)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "{} failed; see output above",
            script.display()
        ))
        .into());
    }
    Ok(())
}
