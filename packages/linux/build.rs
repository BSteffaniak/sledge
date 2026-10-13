use std::{env, fs, path::PathBuf};
fn embed(output: &std::path::Path, kind: &str) {
    let path_key = format!("SLEDGE_HYPRLAND_{kind}_PATH");
    let pin_key = format!("SLEDGE_HYPRLAND_{kind}_SHA256");
    println!("cargo:rerun-if-env-changed={path_key}");
    println!("cargo:rerun-if-env-changed={pin_key}");
    let (bytes, pin) = match (env::var_os(&path_key), env::var(&pin_key).ok()) {
        (None, None) => (Vec::new(), String::new()),
        (Some(path), Some(pin)) => {
            let path = PathBuf::from(path);
            assert!(path.is_absolute(), "input path must be absolute");
            assert!(
                pin.len() == 64
                    && pin
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid SHA256 pin"
            );
            let metadata = fs::metadata(&path).expect("input metadata");
            assert!(
                metadata.is_file() && metadata.len() <= 65536,
                "input must be bounded regular file"
            );
            println!("cargo:rerun-if-changed={}", path.display());
            (fs::read(path).expect("input read"), pin)
        }
        _ => panic!("input path and SHA256 must both be set or unset"),
    };
    fs::write(
        output.join(format!("hyprland-{}.json", kind.to_ascii_lowercase())),
        bytes,
    )
    .expect("embed input");
    println!("cargo:rustc-env=SLEDGE_EMBEDDED_{kind}_SHA256={pin}");
}
fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    embed(&output, "CATALOGUE");
    embed(&output, "ACCEPTANCE");
}
