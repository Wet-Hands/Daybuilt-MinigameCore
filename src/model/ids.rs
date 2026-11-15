//! Id validation. Ids end up in file names, placeholder names and command arguments,
//! so they are kept to a small character set.

/// Arena ids: `[a-z0-9_-]`, 1–32 characters (also the file name).
pub fn arena(id: &str) -> Result<(), String> {
    check(id, 32, |c| c == '_' || c == '-', "arena id")
}

/// Mode ids: `[a-z0-9]`, 1–24 characters, starting with a letter. No `_` because
/// placeholders split on it (`%mg_<mode>_wins%`).
pub fn mode(id: &str) -> Result<(), String> {
    check(id, 24, |_| false, "mode id")?;
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(format!("mode id '{id}' must start with a letter"));
    }
    if RESERVED_MODES.contains(&id) {
        return Err(format!("mode id '{id}' is reserved"));
    }
    Ok(())
}

/// Words that placeholder names use after the namespace, so no mode may take them.
pub const RESERVED_MODES: &[&str] = &["game", "stat", "all"];

/// Format, team, kit, region, group and point ids: `[a-z0-9_]`, 1–24 characters.
pub fn name(id: &str, what: &str) -> Result<(), String> {
    check(id, 24, |c| c == '_', what)
}

/// Kit ids also allow `-`.
pub fn kit(id: &str) -> Result<(), String> {
    check(id, 32, |c| c == '_' || c == '-', "kit id")
}

fn check(id: &str, max: usize, extra: impl Fn(char) -> bool, what: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > max {
        return Err(format!("{what} '{id}' must be 1-{max} characters"));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra(c))
    {
        return Err(format!(
            "{what} '{id}' may only contain lowercase letters, digits{}",
            if extra('_') && extra('-') {
                ", '_' and '-'"
            } else if extra('_') {
                " and '_'"
            } else {
                ""
            }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert!(arena("castle_2-b").is_ok());
        assert!(arena("Castle").is_err());
        assert!(mode("bedwars").is_ok());
        assert!(mode("bed_wars").is_err());
        assert!(mode("1v1").is_err());
        assert!(mode("game").is_err());
        assert!(name("solo_8", "format").is_ok());
        assert!(name("", "format").is_err());
    }
}
