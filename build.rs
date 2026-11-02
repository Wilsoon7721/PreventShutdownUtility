fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_os == "windows" {
        if target_env == "msvc" {
            println!("cargo:rustc-link-arg-bins=/SWAPRUN:CD");
            println!("cargo:rustc-link-arg-bins=/SWAPRUN:NET");
        } else {
            println!(
                "cargo:warning=Non-MSVC Windows target: the EXE will not have the run-from-swap flag, so unplugging the drive it was started from may crash it."
            );
        }
    }
}
