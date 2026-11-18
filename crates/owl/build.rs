//! Bakes the libchat rev from Cargo.lock and flock's own git commit into the
//! binary, so every owl reports exactly what it runs.

fn main() {
    let lock = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    println!("cargo:rerun-if-changed={lock}");
    let rev = std::fs::read_to_string(lock)
        .ok()
        .and_then(|text| {
            let block = text
                .split("[[package]]")
                .find(|b| b.contains("name = \"logos-chat\""))?;
            let source = block.lines().find(|l| l.starts_with("source = "))?;
            Some(source.rsplit('#').next()?.trim_end_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=LIBCHAT_REV={rev}");

    // The Docker build has no .git: it passes OWL_REV in instead.
    println!("cargo:rerun-if-env-changed=OWL_REV");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let owl_rev = std::env::var("OWL_REV").ok().or_else(|| {
        let head = git(&["rev-parse", "--short=12", "HEAD"])?;
        let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
        Some(if dirty { format!("{head}-dirty") } else { head })
    });
    println!(
        "cargo:rustc-env=OWL_REV={}",
        owl_rev.unwrap_or_else(|| "unknown".into())
    );
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.git");
    println!("cargo:rerun-if-changed={root}/HEAD");
    println!("cargo:rerun-if-changed={root}/index");
}
