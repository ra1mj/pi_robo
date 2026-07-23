use pi_cli::{CliParseErrorKind, parse_args};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn parses_every_milestone_one_option_and_alias() {
    let parsed = parse_args(&strings(&[
        "-p",
        "--mode",
        "json",
        "--provider",
        "openai",
        "--model",
        "gpt-4.1",
        "--api-key",
        "secret",
        "--thinking",
        "high",
        "--system-prompt",
        "system",
        "--append-system-prompt",
        "first",
        "--append-system-prompt",
        "second",
        "--skill",
        "skill.md",
        "-ns",
        "-nc",
        "--session-dir",
        "sessions",
        "-n",
        "named",
        "-t",
        "read,bash",
        "-xt",
        "bash",
        "-a",
        "--offline",
        "@prompt.md",
        "hello",
    ]))
    .expect("supported options must parse");

    assert!(parsed.print);
    assert_eq!(parsed.provider.as_deref(), Some("openai"));
    assert_eq!(parsed.append_system_prompt, ["first", "second"]);
    assert_eq!(parsed.tools.enabled, ["read"]);
    assert_eq!(parsed.trust_override, Some(true));
    assert_eq!(parsed.files.len(), 1);
    assert_eq!(parsed.messages, ["hello"]);

    for alias in [
        &["-h"][..],
        &["-v"][..],
        &["-c", "-p"][..],
        &["-nt", "-p"][..],
        &["-na", "-p"][..],
    ] {
        parse_args(&strings(alias)).expect("supported alias must parse");
    }
}

#[test]
fn rejects_missing_values_empty_lists_and_invalid_values() {
    for arguments in [
        &["--provider"][..],
        &["--mode", "interactive"][..],
        &["--thinking", "ultra"][..],
        &["--name", ""][..],
        &["--tools", ","][..],
        &["--tools", "read,grep"][..],
        &["--session-id", "-bad"][..],
        &["@"][..],
    ] {
        let error = parse_args(&strings(arguments)).expect_err("invalid input must fail");
        assert_eq!(error.kind, CliParseErrorKind::Input, "{arguments:?}");
    }
}

#[test]
fn rejects_session_and_trust_conflicts() {
    for arguments in [
        &["--continue", "--session", "one.jsonl"][..],
        &["--session", "one.jsonl", "--session-id", "exact"][..],
        &["--no-session", "--continue"][..],
        &["--approve", "--no-approve"][..],
    ] {
        let error = parse_args(&strings(arguments)).expect_err("conflict must fail");
        assert_eq!(error.kind, CliParseErrorKind::Input, "{arguments:?}");
    }
}

#[test]
fn distinguishes_deferred_and_unknown_options() {
    for option in [
        "--resume",
        "-r",
        "--fork",
        "--models",
        "--no-builtin-tools",
        "-nbt",
        "--extension",
        "-e",
        "--no-extensions",
        "-ne",
        "--prompt-template",
        "--no-prompt-templates",
        "-np",
        "--theme",
        "--no-themes",
        "--export",
        "--tree",
        "--verbose",
    ] {
        let error = parse_args(&strings(&[option])).expect_err("deferred option must fail");
        assert_eq!(error.kind, CliParseErrorKind::Unsupported, "{option}");
        assert!(error.message.contains("milestone 1"));
    }

    for command in ["install", "remove", "uninstall", "update", "list", "config"] {
        let error = parse_args(&strings(&[command])).expect_err("deferred command must fail");
        assert_eq!(error.kind, CliParseErrorKind::Unsupported, "{command}");
    }

    let rpc = parse_args(&strings(&["--mode", "rpc"])).expect_err("RPC is deferred");
    assert_eq!(rpc.kind, CliParseErrorKind::Unsupported);
    for option in ["--future-flag", "---future-flag", "-z"] {
        let unknown = parse_args(&strings(&[option])).expect_err("unknown option must fail");
        assert_eq!(unknown.kind, CliParseErrorKind::Unknown, "{option}");
    }
}

#[test]
fn tool_filter_precedence_matches_typescript_composition() {
    let parsed = parse_args(&strings(&[
        "--no-tools",
        "--tools",
        "read,bash",
        "--exclude-tools",
        "bash",
    ]))
    .expect("tool filters must parse");
    assert_eq!(parsed.tools.enabled, ["read"]);

    let parsed = parse_args(&strings(&["--exclude-tools", "edit"])).expect("denylist must parse");
    assert_eq!(parsed.tools.enabled, ["read", "bash", "write"]);
}
