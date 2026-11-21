use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let hash = Command::new("git")
        .args(["describe", "--always", "--dirty", "--abbrev=8"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!value.is_empty()).then_some(value)
        })
        .unwrap_or_else(|| "unknown".to_owned());

    println!("cargo:rustc-env=CLOSE_APP_BUILD_HASH={hash}");
}
