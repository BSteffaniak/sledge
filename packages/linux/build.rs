use std::{env, fs, path::PathBuf};
fn main() {
    for name in [
        "SLEDGE_HYPRLAND_CATALOGUE_PATH",
        "SLEDGE_HYPRLAND_CATALOGUE_SHA256",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let (bytes, pin) = match (
        env::var_os("SLEDGE_HYPRLAND_CATALOGUE_PATH"),
        env::var("SLEDGE_HYPRLAND_CATALOGUE_SHA256").ok(),
    ) {
        (None, None) => (Vec::new(), String::new()),
        (Some(path), Some(pin)) => {
            let path = PathBuf::from(path);
            assert!(path.is_absolute(), "catalogue path must be absolute");
            assert!(
                pin.len() == 64
                    && pin
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid catalogue SHA256 pin"
            );
            let metadata = fs::metadata(&path).expect("catalogue metadata");
            assert!(
                metadata.is_file() && metadata.len() <= 65536,
                "catalogue must be bounded regular file"
            );
            println!("cargo:rerun-if-changed={}", path.display());
            (fs::read(path).expect("catalogue read"), pin)
        }
        _ => panic!("catalogue path and SHA256 pin must both be set or unset"),
    };
    fs::write(output.join("hyprland-catalogue.json"), bytes).expect("embed catalogue");
    println!("cargo:rustc-env=SLEDGE_EMBEDDED_CATALOGUE_SHA256={pin}");
}
