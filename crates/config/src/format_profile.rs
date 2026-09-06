use taplo::formatter;

#[must_use]
pub fn format_toml(input: &str) -> String {
    let options = formatter::Options {
        indent_string: "    ".to_string(),
        ..Default::default()
    };
    formatter::format(input, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_profiles_with_four_space_indentation() {
        let input = "[section]\nkey = \"value\"\n";

        assert_eq!(format_toml(input), input);
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = format_toml("[section]\nkey=\"value\"\n");

        assert_eq!(format_toml(&once), once);
    }
}
