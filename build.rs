use std::env;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=vendor/pawnio/windows");
    println!("cargo:rerun-if-changed=config.example.toml");

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    if let Err(error) = copy_pawnio_bundle() {
        panic!("failed to bundle PawnIO runtime assets: {error}");
    }

    if let Err(error) = copy_config_example() {
        panic!("failed to bundle config example: {error}");
    }
}

fn copy_pawnio_bundle() -> io::Result<()> {
    let source_dir = PathBuf::from("vendor").join("pawnio").join("windows");

    if !source_dir.exists() {
        return Ok(());
    }

    let profile_dir = cargo_profile_dir()?;
    let destination_dir = profile_dir.join("pawnio").join("windows");

    copy_dir_recursive(&source_dir, &destination_dir)
}

fn copy_config_example() -> io::Result<()> {
    let source = PathBuf::from("config.example.toml");
    if !source.exists() {
        return Ok(());
    }

    let profile_dir = cargo_profile_dir()?;
    copy_file_if_needed(&source, &profile_dir.join("config.example.toml"))
}

fn cargo_profile_dir() -> io::Result<PathBuf> {
    let out_dir =
        PathBuf::from(env::var("OUT_DIR").map_err(|error| io::Error::other(error.to_string()))?);
    let profile = env::var("PROFILE").map_err(|error| io::Error::other(error.to_string()))?;

    out_dir
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile.as_str()))
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("unable to locate Cargo profile output directory"))
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination_path = destination.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &destination_path)?;
        } else if file_type.is_file() {
            copy_file_if_needed(&entry.path(), &destination_path)?;
        }
    }

    Ok(())
}

fn copy_file_if_needed(source: &Path, destination: &Path) -> io::Result<()> {
    if files_match(source, destination)? {
        return Ok(());
    }

    match fs::copy(source, destination) {
        Ok(_) => Ok(()),
        Err(error) if is_file_in_use_error(&error) => {
            println!(
                "cargo:warning=skipped updating {} because it is currently in use",
                destination.display()
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn files_match(source: &Path, destination: &Path) -> io::Result<bool> {
    let source_metadata = fs::metadata(source)?;
    let destination_metadata = match fs::metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };

    if source_metadata.len() != destination_metadata.len() {
        return Ok(false);
    }

    Ok(fs::read(source)? == fs::read(destination)?)
}

fn is_file_in_use_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(32)
}
