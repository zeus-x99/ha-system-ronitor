pub fn load_env_files() {
    let _ = dotenvy::from_filename(".env");
    let _ = dotenvy::from_filename_override(".env.local");
}
