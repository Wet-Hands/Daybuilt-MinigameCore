//! Text helpers: `&` colour codes, team colours, simple `{placeholder}` filling.

/// Replaces `&x` colour/format codes with `§x`. `&&` gives a literal `&`.
pub fn legacy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.peek() {
                Some('&') => {
                    out.push('&');
                    chars.next();
                }
                Some(n) if "0123456789abcdefklmnorABCDEFKLMNOR".contains(*n) => {
                    out.push('§');
                    out.push(n.to_ascii_lowercase());
                    chars.next();
                }
                _ => out.push('&'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Removes `&x` and `§x` codes (for console text and length checks).
pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '&' || c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// The `&` code for a colour name (Minecraft's 16 named colours). Unknown names give white.
pub fn color_code(color: &str) -> &'static str {
    match color.to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "black" => "&0",
        "dark_blue" => "&1",
        "dark_green" => "&2",
        "dark_aqua" | "cyan" => "&3",
        "dark_red" => "&4",
        "dark_purple" | "purple" => "&5",
        "gold" | "orange" => "&6",
        "gray" | "grey" => "&7",
        "dark_gray" | "dark_grey" => "&8",
        "blue" => "&9",
        "green" | "lime" => "&a",
        "aqua" | "light_blue" => "&b",
        "red" => "&c",
        "light_purple" | "pink" | "magenta" => "&d",
        "yellow" => "&e",
        _ => "&f",
    }
}

/// True if `color` is one of the names `color_code` knows.
pub fn is_color(color: &str) -> bool {
    color_code(color) != "&f" || matches!(color, "white")
}

/// Fills `{key}` placeholders. Unknown keys are left as written.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in values {
        let key = format!("{{{k}}}");
        if out.contains(&key) {
            out = out.replace(&key, v);
        }
    }
    out
}

/// `mm:ss` for a number of seconds.
pub fn clock(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_codes() {
        assert_eq!(legacy("&aHi &&b &zx"), "§aHi &b &zx");
        assert_eq!(strip("§aHi &bthere"), "Hi there");
        assert_eq!(color_code("Dark-Red"), "&4");
        assert!(is_color("white"));
        assert!(!is_color("plaid"));
        assert_eq!(fill("{a}-{b}-{c}", &[("a", "1"), ("b", "2")]), "1-2-{c}");
        assert_eq!(clock(125), "02:05");
    }
}
