use pi_test_support::{CompatibilityState, fixture_path, validate_compatibility_catalog};
use serde_json::Value;
use std::error::Error;
use std::fs;

const PROVIDER_FIXTURES: [(&str, &str); 5] = [
    ("openai_chat", "rust/fixtures/providers/openai-chat.json"),
    (
        "openai_responses",
        "rust/fixtures/providers/openai-responses.json",
    ),
    (
        "anthropic_messages",
        "rust/fixtures/providers/anthropic-messages.json",
    ),
    (
        "google_generative_language",
        "rust/fixtures/providers/google-generative-language.json",
    ),
    ("faux", "rust/fixtures/providers/faux.json"),
];

#[test]
fn verified_provider_fixtures_have_complete_evidence_sections() -> Result<(), Box<dyn Error>> {
    for (protocol, relative_path) in PROVIDER_FIXTURES {
        let fixture: Value =
            serde_json::from_str(&fs::read_to_string(fixture_path(relative_path)?)?)?;
        assert_eq!(fixture["schemaVersion"], 1);
        assert_eq!(fixture["protocol"], protocol);
        assert_non_empty_array(&fixture, "requestCases", relative_path);
        assert_non_empty_array(&fixture, "streamCases", relative_path);
        assert_non_empty_array(&fixture, "errorCases", relative_path);
        assert!(fixture["allowedNondeterminism"].is_array());
        assert!(fixture["unsupported"].is_array());

        for error_case in fixture["errorCases"].as_array().expect("checked array") {
            assert!(error_case["category"].is_string());
            assert!(error_case["retryable"].is_boolean());
            assert!(error_case.get("retryAfterMs").is_some());
        }
    }
    Ok(())
}

#[test]
fn provider_catalog_rows_are_verified() -> Result<(), Box<dyn Error>> {
    let catalog =
        validate_compatibility_catalog(&fixture_path("rust/fixtures/compatibility.json")?)
            .map_err(|errors| errors.join("\n"))?;
    let provider_entries: Vec<_> = catalog
        .entries
        .iter()
        .filter(|entry| entry.owner == "pi-provider")
        .collect();

    assert_eq!(provider_entries.len(), PROVIDER_FIXTURES.len());
    assert!(
        provider_entries
            .iter()
            .all(|entry| entry.state == CompatibilityState::Verified)
    );
    Ok(())
}

fn assert_non_empty_array(fixture: &Value, field: &str, path: &str) {
    assert!(
        fixture[field]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{path} must contain {field}"
    );
}
