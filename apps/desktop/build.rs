use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BuildConfig {
    microsoft_client_id: String,
    _comment: Option<String>,
}

fn main() {
    let path =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest directory"))
            .join("../../build-config.local.json");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-env-changed=MEMORIA_MICROSOFT_CLIENT_ID");

    let client_id = match env::var("MEMORIA_MICROSOFT_CLIENT_ID") {
        Ok(value) if !value.trim().is_empty() => value,
        Err(env::VarError::NotUnicode(_)) => {
            panic!("MEMORIA_MICROSOFT_CLIENT_ID must be valid Unicode")
        }
        _ => match fs::read(&path) {
            Ok(bytes) => {
                let config: BuildConfig = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                    panic!(
                        "build-config.local.json is invalid (line {}, column {}); see build-config.example.json",
                        error.line(),
                        error.column()
                    )
                });
                config.microsoft_client_id
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => panic!("cannot read build-config.local.json: {error}"),
        },
    };

    let client_id = client_id.trim();
    if !client_id.is_empty() {
        uuid::Uuid::parse_str(client_id).expect(
            "Microsoft client ID must be an application UUID; do not enter a client secret or API key",
        );
    }
    // Always set the value so removing a local ID also clears the embedded default.
    println!("cargo:rustc-env=MEMORIA_MICROSOFT_CLIENT_ID={client_id}");
}
