fn main() {
    /* Slint's embedded bitmap renderer chooses the nearest available glyph
     * size at runtime. Include physical sizes for the iPhone's 2.6087 scale;
     * otherwise even a 34 px logical heading uses a 34 px glyph on its panel. */
    if std::env::var_os("SLINT_FONT_SIZES").is_none() {
        // SAFETY: No threads have been started in this build script yet.
        unsafe { std::env::set_var("SLINT_FONT_SIZES", "31,33,36,41,44,47,50,52,57,60,62,88") };
    }
    let ui = if std::env::var_os("CARGO_FEATURE_FULL_SUITE_UI").is_some() {
        "ui/full_suite.slint"
    } else if std::env::var_os("CARGO_FEATURE_WORKSPACE_UI").is_some() {
        "ui/workspaces.slint"
    } else {
        "ui/notes.slint"
    };
    slint_build::compile_with_config(
        ui,
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer),
    )
    .expect("compile Slint UI");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        let output = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap())
            .join("iphone-libc.o");
        let status = std::process::Command::new("clang")
            .args(["-target", "aarch64-none-elf", "-c", "-O2", "-nostdlib",
                   "-ffreestanding", "-fno-builtin", "iphone-libc.c", "-o"])
            .arg(&output)
            .status()
            .expect("compile iPhone C support");
        assert!(status.success(), "compile iPhone C support");
        println!("cargo:rustc-link-arg-bin=slint_notes_iphone={}", output.display());
        println!("cargo:rustc-link-arg-bin=slint_notes_pixel={}", output.display());
        println!("cargo:rerun-if-changed=iphone-libc.c");
    }
}
