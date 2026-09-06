use config::format_profile::format_toml;

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
