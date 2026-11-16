//! TOML helpers: deep merge of user files over embedded defaults.

use toml::{Table, Value};

/// Merges `over` into `base`: tables merge key by key, everything else is replaced.
pub fn merge(base: &mut Table, over: Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(b)), Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(key, v);
            }
        }
    }
}

/// Parses `user` and merges it over the parsed `default`.
pub fn merged(default: &str, user: Option<&str>) -> Result<Table, String> {
    let mut base: Table = toml::from_str(default).map_err(|e| format!("built-in default: {e}"))?;
    if let Some(text) = user {
        let over: Table = toml::from_str(text).map_err(|e| e.to_string())?;
        merge(&mut base, over);
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge() {
        let t = merged("[a]\nx = 1\ny = 2\n[b]\nz = 3\n", Some("[a]\ny = 5\n")).unwrap();
        assert_eq!(t["a"]["x"].as_integer(), Some(1));
        assert_eq!(t["a"]["y"].as_integer(), Some(5));
        assert_eq!(t["b"]["z"].as_integer(), Some(3));
    }
}
