fn main() {
    // ggml-metal uses `@available(macOS 15, *)` checks (residency sets), which
    // compile to clang's `__isPlatformVersionAtLeast` builtin. rustc links with
    // `-nodefaultlibs`, so pull in clang's builtins library explicitly for any
    // binary that links this crate.
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("clang")
            .arg("--print-resource-dir")
            .output()
            .expect("run clang --print-resource-dir");
        let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!("cargo:rustc-link-search={resource_dir}/lib/darwin");
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
}
