//! Where a downloaded Z3 static library is kept between builds.
//!
//! Cargo's `OUT_DIR` changes when `build.rs` changes, so a copy there is
//! downloaded again. This path stays put for one Z3 version and target.

use std::path::{Path, PathBuf};

pub fn cache_root_from_env(
    cache_dir: Option<&str>,
    xdg_cache_home: Option<&str>,
    home: Option<&str>,
    userprofile: Option<&str>,
    local_app_data: Option<&str>,
    out_dir: &Path,
) -> PathBuf {
    if let Some(dir) = nonempty(cache_dir) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = nonempty(xdg_cache_home) {
        return PathBuf::from(xdg).join("z3-sys");
    }
    if let Some(home) = nonempty(home).or_else(|| nonempty(userprofile)) {
        return PathBuf::from(home).join(".cache").join("z3-sys");
    }
    if let Some(local) = nonempty(local_app_data) {
        return PathBuf::from(local).join("z3-sys");
    }
    out_dir.join("z3-sys-cache")
}

pub fn cached_static_lib(
    root: &Path,
    version: &str,
    os: &str,
    arch: &str,
    target_os: &str,
) -> PathBuf {
    let mut dir = std::string::String::from("z3-");
    dir.push_str(version);
    dir.push('-');
    dir.push_str(os);
    dir.push('-');
    dir.push_str(arch);
    root.join(dir).join(static_lib_rel(target_os))
}

fn static_lib_rel(target_os: &str) -> &'static str {
    if target_os == "windows" {
        "bin/libz3.lib"
    } else {
        "bin/libz3.a"
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_cache_dir_wins() {
        let root = cache_root_from_env(
            Some("/opt/z3"),
            Some("/xdg"),
            Some("/home/builder"),
            None,
            None,
            Path::new("/out"),
        );
        assert_eq!(root, Path::new("/opt/z3"));
    }

    #[test]
    fn home_cache_is_the_default() {
        let root = cache_root_from_env(None, None, Some("/home/builder"), None, None, Path::new("/out"));
        assert_eq!(root, Path::new("/home/builder/.cache/z3-sys"));
    }

    #[test]
    fn empty_env_values_are_ignored() {
        let root = cache_root_from_env(
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("/local"),
            Path::new("/out"),
        );
        assert_eq!(root, Path::new("/local/z3-sys"));
    }

    #[test]
    fn lib_path_includes_version_and_target() {
        let lib = cached_static_lib(Path::new("/cache"), "5.1.0", "glibc", "x64", "linux");
        assert_eq!(lib, Path::new("/cache/z3-5.1.0-glibc-x64/bin/libz3.a"));
        let win = cached_static_lib(Path::new("/cache"), "5.1.0", "win", "x64", "windows");
        assert_eq!(win, Path::new("/cache/z3-5.1.0-win-x64/bin/libz3.lib"));
    }
}
