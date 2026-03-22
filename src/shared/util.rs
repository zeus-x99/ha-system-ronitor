use std::path::Path;

pub fn disk_component_id(mount_point: &str, fallback_name: &str) -> String {
    let preferred = Path::new(mount_point)
        .components()
        .filter_map(|segment| {
            let value = segment.as_os_str().to_string_lossy().into_owned();
            if value == "\\" || value == "/" || value.is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .next_back()
        .unwrap_or_else(|| fallback_name.to_string());

    let slug = slugify(&preferred);
    if slug.is_empty() {
        slugify(fallback_name)
    } else {
        slug
    }
}

pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            slug.push('_');
            previous_was_separator = true;
        }
    }

    slug.trim_matches('_').to_string()
}
