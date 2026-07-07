use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // Unique per compile so static asset URLs (?v=...) change on every build,
    // not just on version bumps. No rerun-if-changed directives: cargo's
    // default is to rerun this script whenever any package file changes,
    // which covers templates/ and static/ too.
    let build_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    println!("cargo:rustc-env=BUILD_ID={}", build_id);
}
