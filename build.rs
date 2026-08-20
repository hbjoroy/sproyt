use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-env-changed=SPROYT_FRONTEND_PREBUILT");
    for path in [
        "frontend/package.json",
        "frontend/package-lock.json",
        "frontend/tsconfig.json",
        "frontend/src",
        "frontend/scripts",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
    let frontend_dir = Path::new(&manifest_dir).join("frontend");
    let output_dir = PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let app_bundle = output_dir.join("app.js");
    let client_store = output_dir.join("client-store.js");
    if env::var_os("SPROYT_FRONTEND_PREBUILT").is_some() {
        let generated_app = frontend_dir.join("dist/app.js");
        let generated_client_store = frontend_dir.join("dist/client-store.js");
        assert!(
            generated_app.is_file() && generated_client_store.is_file(),
            "SPROYT_FRONTEND_PREBUILT requires frontend/dist/app.js and frontend/dist/client-store.js"
        );
        fs::copy(generated_app, app_bundle).expect("copy prebuilt app bundle into Cargo OUT_DIR");
        fs::copy(generated_client_store, client_store)
            .expect("copy prebuilt legacy client-store into Cargo OUT_DIR");
        return;
    }
    if !frontend_dir.join("node_modules").is_dir() {
        panic!(
            "frontend dependencies are missing; run `npm --prefix frontend ci` before Cargo commands"
        );
    }

    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(["--prefix", "frontend", "run", "build"])
        .current_dir(&manifest_dir)
        .env("SPROYT_FRONTEND_OUT_DIR", &output_dir)
        .status()
        .expect("failed to start npm; install the Node.js version in .node-version");
    assert!(
        status.success(),
        "frontend build failed; run `npm --prefix frontend run check` for TypeScript diagnostics"
    );
    assert!(
        app_bundle.is_file() && client_store.is_file(),
        "frontend build did not create app.js and client-store.js in Cargo OUT_DIR"
    );
}
