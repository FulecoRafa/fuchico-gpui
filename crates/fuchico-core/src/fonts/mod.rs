use font_kit::source::SystemSource;

pub fn fonts_list_system() -> Result<Vec<String>, String> {
    let mut families = SystemSource::new()
        .all_families()
        .map_err(|e| e.to_string())?;
    families.sort();
    families.dedup();
    Ok(families)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_fonts_are_sorted_and_deduplicated() {
        let families = fonts_list_system().unwrap();
        // Byte-order sort, then dedup: strictly increasing.
        assert!(families.windows(2).all(|w| w[0] < w[1]), "not strictly sorted");
        #[cfg(target_os = "macos")]
        assert!(!families.is_empty(), "macOS always ships system fonts");
    }
}
