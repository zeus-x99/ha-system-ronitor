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
    candidate_config_directories_from(
        additional_directories,
        env::current_dir().ok(),
        env::current_exe()
            .ok()
            .and_then(|current_exe| current_exe.parent().map(Path::to_path_buf)),
    )
}

fn candidate_config_directories_from<I>(
    additional_directories: I,
    current_dir: Option<PathBuf>,
    executable_dir: Option<PathBuf>,
) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for directory in additional_directories {
        push_unique(&mut candidates, &mut seen, directory);
    }

    if let Some(current_dir) = current_dir {
        push_unique(&mut candidates, &mut seen, current_dir);
    }

    if let Some(executable_dir) = executable_dir {
        push_unique(&mut candidates, &mut seen, executable_dir);
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

#[cfg(test)]
mod tests {
    use super::candidate_config_directories_from;
    use std::path::PathBuf;

    #[test]
    fn prefers_explicit_then_current_dir_then_executable_dir() {
        let candidates = candidate_config_directories_from(
            [PathBuf::from("D:/explicit-config")],
            Some(PathBuf::from("C:/workspace/project")),
            Some(PathBuf::from("C:/workspace/project/target/debug")),
        );

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("D:/explicit-config"),
                PathBuf::from("C:/workspace/project"),
                PathBuf::from("C:/workspace/project/target/debug"),
            ]
        );
    }

    #[test]
    fn does_not_scan_parent_directories_of_executable() {
        let candidates = candidate_config_directories_from(
            std::iter::empty::<PathBuf>(),
            Some(PathBuf::from("C:/workspace/project")),
            Some(PathBuf::from("C:/workspace/project/target/debug")),
        );

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("C:/workspace/project"),
                PathBuf::from("C:/workspace/project/target/debug"),
            ]
        );
        assert!(!candidates.contains(&PathBuf::from("C:/workspace")));
        assert!(!candidates.contains(&PathBuf::from("C:/workspace/project/target")));
    }
}
