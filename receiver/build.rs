//! Compile only the narrow native PipeWire bridge, using the configured C compiler.
//! The offline build needs neither system headers nor pkg-config. No bindgen is used.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/capture/pw_bridge.c");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if env::var_os("CARGO_FEATURE_LIVE_CAPTURE").is_none() {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "live-capture requires Linux"
    );
    let flags = Command::new("pkg-config")
        .args(["--cflags", "--libs", "libpipewire-0.3"])
        .output()
        .expect("install pkg-config and libpipewire-0.3-dev");
    assert!(
        flags.status.success(),
        "libpipewire-0.3-dev unavailable: {}",
        String::from_utf8_lossy(&flags.stderr)
    );
    let flags = String::from_utf8(flags.stdout).unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("pw_bridge.o");
    let mut cc = Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()));
    cc.args([
        "-std=gnu11",
        "-O2",
        "-fPIC",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wno-unused-parameter",
    ]);
    for flag in flags
        .split_whitespace()
        .filter(|s| !s.starts_with("-l") && !s.starts_with("-L"))
    {
        cc.arg(flag);
    }
    assert!(
        cc.args(["-c", "src/capture/pw_bridge.c", "-o"])
            .arg(&object)
            .status()
            .expect("C compiler not found")
            .success(),
        "PipeWire bridge compilation failed"
    );
    assert!(
        Command::new(env::var_os("AR").unwrap_or_else(|| "ar".into()))
            .arg("crs")
            .arg(out.join("libvmt_pw.a"))
            .arg(object)
            .status()
            .expect("ar not found")
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=vmt_pw");
    for flag in flags.split_whitespace() {
        if let Some(name) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={name}");
        }
        if let Some(dir) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        }
    }
}
