fn main() {
    let timezone_output = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("timezone.bin");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none")
        && std::env::var_os("CARGO_FEATURE_UTM_PD").is_some()
    {
        let status = std::process::Command::new("python3")
            .args(["timezone_table.py"]).arg(&timezone_output)
            .status().expect("generate host timezone transitions");
        assert!(status.success(), "generate host timezone transitions");
        println!("cargo:rerun-if-changed=timezone_table.py");
        println!("cargo:rerun-if-changed=/etc/localtime");
        println!("cargo:rerun-if-env-changed=NEXATRODE_HOST_TIMEZONE_SHA256");
    } else {
        std::fs::write(&timezone_output, []).expect("create empty timezone table");
    }
    /* Slint's embedded bitmap renderer chooses the nearest available glyph
     * size at runtime. Include physical sizes for the iPhone's 2.6087 scale;
     * otherwise even a 34 px logical heading uses a 34 px glyph on its panel. */
    if std::env::var_os("SLINT_FONT_SIZES").is_none() {
        // SAFETY: No threads have been started in this build script yet.
        unsafe { std::env::set_var("SLINT_FONT_SIZES", "31,33,36,41,44,47,50,52,57,60,62,88") };
    }
    let ui = if std::env::var_os("CARGO_FEATURE_DESKTOP_REAL_APP").is_some() {
        "ui/full_suite.slint"
    } else if std::env::var_os("CARGO_FEATURE_DESKTOP_SHELL_UI").is_some() {
        "ui/desktop_shell.slint"
    } else if std::env::var_os("CARGO_FEATURE_DESKTOP_SHELL_APP").is_some() {
        "ui/desktop_app.slint"
    } else if std::env::var_os("CARGO_FEATURE_DESKTOP_LOCK_UI").is_some() {
        "ui/lifecycle_lock.slint"
    } else if std::env::var_os("CARGO_FEATURE_FULL_SUITE_UI").is_some() {
        "ui/full_suite.slint"
    } else if std::env::var_os("CARGO_FEATURE_WORKSPACE_UI").is_some() {
        "ui/workspaces.slint"
    } else {
        "ui/notes.slint"
    };
    let mut ui_config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    if std::env::var_os("CARGO_FEATURE_DESKTOP_TRIO_LOCK_AUTH").is_some() {
        ui_config=ui_config.with_scale_factor(2.0);
    }
    if std::env::var_os("CARGO_FEATURE_DESKTOP_SHELL_UI").is_some()
        || std::env::var_os("CARGO_FEATURE_DESKTOP_SHELL_APP").is_some()
        || std::env::var_os("CARGO_FEATURE_DESKTOP_REAL_APP").is_some()
    {
        // Embedded SVGs and font glyphs must be rasterized at the same density
        // as the guest window. Phone adapters retain their existing behavior.
        let scale = if std::env::var_os("CARGO_FEATURE_DESKTOP_SHELL_2X").is_some()
            || std::env::var_os("CARGO_FEATURE_DESKTOP_REAL_APP").is_some() { 2.0 } else { 1.0 };
        ui_config = ui_config.with_scale_factor(scale);
    }
    slint_build::compile_with_config(ui, ui_config)
    .expect("compile Slint UI");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        let output =
            std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("iphone-libc.o");
        let status = std::process::Command::new("clang")
            .args([
                "-target",
                "aarch64-none-elf",
                "-mstrict-align",
                "-c",
                "-O2",
                "-nostdlib",
                "-ffreestanding",
                "-fno-builtin",
                "iphone-libc.c",
                "-o",
            ])
            .arg(&output)
            .status()
            .expect("compile iPhone C support");
        assert!(status.success(), "compile iPhone C support");
        println!(
            "cargo:rustc-link-arg-bin=slint_notes_iphone={}",
            output.display()
        );
        println!(
            "cargo:rustc-link-arg-bin=slint_notes_pixel={}",
            output.display()
        );
        // slint_notes_utm links personal-os-rust-server, which already exports
        // memmove and memcmp. Linking this object there duplicates those symbols.
        println!("cargo:rerun-if-changed=iphone-libc.c");
        compile_skia_guest();
    }
}

fn compile_skia_guest() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let skia_dir = manifest.join("skia");
    let source = manifest.join("../../.cache/skia");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("skia-guest");
    std::fs::create_dir_all(&out).expect("create Skia guest objects");
    let wrapper = skia_dir.join("bin/clang++-aarch64.sh");
    let cc = skia_dir.join("bin/clang-aarch64.sh");
    let libcxx = manifest.join("../../.cache/llvm-project/libcxx/src");
    let units = [
        "string.cpp",
        "memory.cpp",
        "locale.cpp",
        "ios.cpp",
        "iostream.cpp",
        "ostream.cpp",
        "verbose_abort.cpp",
        "new.cpp",
        "stdexcept.cpp",
        "system_error.cpp",
        "ios.instantiations.cpp",
        "new_helpers.cpp",
        "new_handler.cpp",
        "error_category.cpp",
        "exception.cpp",
        "hash.cpp",
        "call_once.cpp",
        "functional.cpp",
    ];
    for unit in units {
        let object = out.join(unit.replace(".cpp", ".o"));
        let status = std::process::Command::new(&wrapper)
            .args([
                "-std=c++23",
                "-fno-exceptions",
                "-fno-rtti",
                "-O2",
                "-D_LIBCPP_BUILDING_LIBRARY",
                "-I",
            ])
            .arg(&libcxx)
            .arg("-c")
            .arg(libcxx.join(unit))
            .arg("-o")
            .arg(&object)
            .status()
            .expect("compile libc++ for Skia");
        assert!(status.success(), "compile libc++ {unit}");
        println!("cargo:rustc-link-arg-bin=slint_notes_utm={}", object.display());
    }
    let local_units = [
        ("runtime.cc", true),
        ("locale_stubs.c", false),
        ("skia_label.cc", true),
    ];
    for (unit, cxx) in local_units {
        let object = out.join(unit.replace('.', "_") + ".o");
        let compiler = if cxx { &wrapper } else { &cc };
        let mut command = std::process::Command::new(compiler);
        if cxx {
            command.args(["-std=c++20", "-fno-exceptions", "-fno-rtti", "-O2", "-I"]).arg(&source);
        } else {
            command.arg("-O2");
        }
        let status = command
            .arg("-c")
            .arg(skia_dir.join(unit))
            .arg("-o")
            .arg(&object)
            .status()
            .expect("compile Skia guest support");
        assert!(status.success(), "compile Skia guest support {unit}");
        println!("cargo:rustc-link-arg-bin=slint_notes_utm={}", object.display());
        println!("cargo:rerun-if-changed={}", skia_dir.join(unit).display());
    }
    let setjmp = out.join("setjmp.o");
    let status = std::process::Command::new("/opt/homebrew/opt/llvm/bin/clang")
        .args(["--target=aarch64-unknown-none-elf", "-c"])
        .arg(skia_dir.join("setjmp.S"))
        .arg("-o")
        .arg(&setjmp)
        .status()
        .expect("assemble setjmp");
    assert!(status.success(), "assemble setjmp");
    println!("cargo:rustc-link-arg-bin=slint_notes_utm={}", setjmp.display());
    println!("cargo:rustc-link-arg-bin=slint_notes_utm=--whole-archive");
    let archive = source.join("out/guest/libskia.a");
    println!("cargo:rustc-link-arg-bin=slint_notes_utm={}", archive.display());
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rustc-link-arg-bin=slint_notes_utm=--no-whole-archive");
    println!("cargo:rustc-link-arg-bin=slint_notes_utm=--strip-debug");
    println!("cargo:rerun-if-changed={}", wrapper.display());
    println!("cargo:rerun-if-changed={}", skia_dir.join("setjmp.S").display());
}
