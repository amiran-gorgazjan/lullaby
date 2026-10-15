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
    nice_plug_xtask::main()?;
    if args
        .first()
        .is_some_and(|arg| arg == "bundle" || arg == "bundle-universal")
    {
        bundle_notices()?;
    }
    Ok(())
}

fn bundle_notices() -> nice_plug_xtask::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let bundles = target.join("bundled");
    let notices = [
        (
            "FONT-LICENSES.txt",
            concat!(
                "Lullaby interface fonts\n\nCormorant Garamond\n",
                include_str!("../../assets/fonts/CormorantGaramond-OFL.txt"),
                "\n\nLato\n",
                include_str!("../../assets/fonts/Lato-OFL.txt"),
            ),
        ),
        (
            "LULLABY-LICENSES.txt",
            concat!(
                "Lullaby project licensing\n\n",
                include_str!("../../LICENSE"),
                "\n\nPreviously distributed MIT baseline\n\n",
                include_str!("../../licenses/MIT-BASELINE.txt"),
            ),
        ),
    ];
    // Companion notices also cover single-file CLAPs on Windows and Linux.
    for (filename, text) in notices {
        std::fs::write(bundles.join(filename), text)?;
    }
    for name in ["Lullaby.vst3", "Lullaby.clap"] {
        let bundle = bundles.join(name);
        if !bundle.is_dir() {
            continue;
        }
        let resources = bundle.join("Contents/Resources");
        std::fs::create_dir_all(&resources)?;
        for (filename, text) in notices {
            std::fs::write(resources.join(filename), text)?;
        }
        // Resource changes invalidate the signature made by the upstream bundler.
        #[cfg(target_os = "macos")]
        if bundle.join("Contents/MacOS").is_dir() {
            let status = std::process::Command::new("codesign")
                .args(["--force", "--sign", "-"])
                .arg(&bundle)
                .status()?;
            if !status.success() {
                return Err(
                    std::io::Error::other("Could not sign bundle with license notices").into(),
                );
            }
        }
    }
    Ok(())
}
