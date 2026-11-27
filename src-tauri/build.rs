include!("src/app_commands.rs");

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&APP_COMMANDS);
    if let Err(error) =
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
    {
        panic!("tauri-build failed: {error:#}");
    }
}
