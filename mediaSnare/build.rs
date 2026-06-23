// build.rs — write config.rs into OUT_DIR
//
// For release builds meson sets APP_ID, VERSION, PKGDATADIR via environment.
// For dev builds (cargo build outside meson) defaults are used.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir    = PathBuf::from(env::var("OUT_DIR").unwrap());
    let app_id     = env::var("APP_ID").unwrap_or_else(|_| "org.archerprojects.mediaSnare".into());
    let version    = env::var("VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());
    let pkgdatadir = env::var("PKGDATADIR").unwrap_or_else(|_| "/usr/share/mediasnare".into());

    let config = format!(
        r#"pub const APP_ID: &str = "{app_id}";
pub const VERSION: &str = "{version}";
pub const PKGDATADIR: &str = "{pkgdatadir}";
pub const RESOURCES_FILE: &str = "{pkgdatadir}/mediasnare.gresource";
"#
    );

    fs::write(out_dir.join("config.rs"), config).unwrap();
}
