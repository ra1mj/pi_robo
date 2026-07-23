use pi_cli::{OutputMode, parse_args, resolve_output_mode};

fn resolve(arguments: &[&str], stdin_is_terminal: bool) -> Result<OutputMode, String> {
    let arguments = arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let parsed = parse_args(&arguments).map_err(|error| error.to_string())?;
    resolve_output_mode(&parsed, stdin_is_terminal).map_err(|error| error.to_string())
}

#[test]
fn explicit_json_wins_over_print() {
    assert_eq!(
        resolve(&["--mode", "json", "-p"], true),
        Ok(OutputMode::Json)
    );
}

#[test]
fn explicit_text_and_piped_stdin_select_text() {
    assert_eq!(resolve(&["--mode", "text"], true), Ok(OutputMode::Text));
    assert_eq!(resolve(&[], false), Ok(OutputMode::Text));
}

#[test]
fn metadata_commands_do_not_require_headless_selection() {
    assert_eq!(resolve(&["--help"], true), Ok(OutputMode::Metadata));
    assert_eq!(resolve(&["--version"], true), Ok(OutputMode::Metadata));
    assert_eq!(resolve(&["--list-models"], true), Ok(OutputMode::Metadata));
}

#[test]
fn bare_tty_and_interactive_like_positional_input_fail_with_guidance() {
    for arguments in [&[][..], &["hello"][..]] {
        let error = resolve(arguments, true).expect_err("interactive invocation must fail");
        assert!(error.contains("add -p or --mode text"));
    }
}
