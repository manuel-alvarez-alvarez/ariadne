use std::process::Command;

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("run git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn main() {
    println!("cargo:rerun-if-env-changed=ARIADNE_RELEASE_REPOSITORY");
    let checkout = git(&["rev-parse", "--show-toplevel"]);
    for path in git(&[
        "-C",
        &checkout,
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "--full-name",
    ])
    .lines()
    {
        println!("cargo:rerun-if-changed={checkout}/{path}");
    }
    for name in ["HEAD", "index", "logs/HEAD"] {
        let path = git(&["rev-parse", "--path-format=absolute", "--git-path", name]);
        println!("cargo:rerun-if-changed={path}");
    }
    println!(
        "cargo:rustc-env=ARIADNE_BUILD_COMMIT={}",
        git(&["rev-parse", "HEAD"])
    );
    println!(
        "cargo:rustc-env=ARIADNE_BUILD_UNCOMMITTED={}",
        !git(&["status", "--porcelain", "--untracked-files=normal"]).is_empty()
    );
    println!("cargo:rustc-env=ARIADNE_BUILD_CHECKOUT={checkout}");
    println!(
        "cargo:rustc-env=ARIADNE_RELEASE_REPOSITORY={}",
        std::env::var("ARIADNE_RELEASE_REPOSITORY").unwrap_or_default()
    );
}
