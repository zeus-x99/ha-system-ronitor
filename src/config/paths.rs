use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

pub fn candidate_config_directories() -> Vec<PathBuf> {
    candidate_config_directories_with(std::iter::empty::<PathBuf>())
}

pub fn candidate_config_directories_with<I>(additional_directories: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for directory in additional_directories {
        push_unique(&mut candidates, &mut seen, directory);
    }

    if let Ok(current_exe) = env::current_exe() {
        for ancestor in current_exe
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            push_unique(&mut candidates, &mut seen, ancestor);
        }
    }

    if let Ok(current_dir) = env::current_dir() {
        push_unique(&mut candidates, &mut seen, current_dir);
    }

    candidates
}

fn push_unique(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    directory: impl AsRef<Path>,
) {
    let directory = directory.as_ref().to_path_buf();
    if seen.insert(directory.clone()) {
        candidates.push(directory);
    }
}
