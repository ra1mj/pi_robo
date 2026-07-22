use pi_protocol::ContractErrorCategory;
use pi_protocol::{
    AssistantMessageEvent, CompletionReason, FailureReason, Message, ModelCatalog,
    PersistedSessionRecord, Settings,
};
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::path::Path;

fn repository_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

#[test]
fn messages_round_trip_without_losing_extensions() -> Result<(), Box<dyn Error>> {
    let source: Value = serde_json::from_str(&fs::read_to_string(repository_path(
        "rust/fixtures/protocol/messages.json",
    ))?)?;
    let messages: Vec<Message> = serde_json::from_value(source.clone())?;
    assert_eq!(serde_json::to_value(messages)?, source);
    Ok(())
}

#[test]
fn assistant_events_match_the_fixture() -> Result<(), Box<dyn Error>> {
    let source: Value = serde_json::from_str(&fs::read_to_string(repository_path(
        "rust/fixtures/protocol/assistant-events.json",
    ))?)?;
    let events: Vec<AssistantMessageEvent> = serde_json::from_value(source.clone())?;
    assert_eq!(serde_json::to_value(&events)?, source);
    assert!(matches!(
        events.first(),
        Some(AssistantMessageEvent::Done {
            reason: CompletionReason::Stop,
            ..
        })
    ));
    assert!(matches!(
        events.get(1),
        Some(AssistantMessageEvent::Error {
            reason: FailureReason::Error,
            ..
        })
    ));
    Ok(())
}

#[test]
fn settings_preserve_unknown_keys() -> Result<(), Box<dyn Error>> {
    let source: Value = serde_json::from_str(&fs::read_to_string(repository_path(
        "rust/fixtures/protocol/settings.json",
    ))?)?;
    let settings: Settings = serde_json::from_value(source.clone())?;
    assert_eq!(serde_json::to_value(settings)?, source);
    Ok(())
}

#[test]
fn session_records_preserve_unknown_fields_and_record_types() -> Result<(), Box<dyn Error>> {
    let content = fs::read_to_string(repository_path("rust/fixtures/sessions/session-v3.jsonl"))?;
    for line in content.lines() {
        let source: Value = serde_json::from_str(line)?;
        let record = PersistedSessionRecord::parse(line)?;
        assert_eq!(record.raw(), &source);
        assert_eq!(record.to_value()?, source);
    }

    let unknown =
        PersistedSessionRecord::parse(r#"{"type":"future_entry","payload":{"must":"survive"}}"#)?;
    assert!(unknown.known().is_none());
    Ok(())
}

#[test]
fn generated_model_catalog_is_typed_and_nonempty() -> Result<(), Box<dyn Error>> {
    let catalog: ModelCatalog = serde_json::from_str(&fs::read_to_string(repository_path(
        "rust/assets/models.json",
    ))?)?;
    assert_eq!(catalog.schema_version, 1);
    assert!(catalog.providers.len() >= 30);
    assert!(
        catalog
            .providers
            .values()
            .map(|provider| provider.models.len())
            .sum::<usize>()
            >= 1000
    );
    Ok(())
}

#[test]
fn malformed_session_records_return_structured_errors() {
    let missing_type =
        PersistedSessionRecord::parse(r#"{"id":"entry-1"}"#).expect_err("type is required");
    assert_eq!(missing_type.category, ContractErrorCategory::InvalidShape);
    assert_eq!(missing_type.path.as_deref(), Some("$.type"));

    let incomplete_header = PersistedSessionRecord::parse(r#"{"type":"session","version":3}"#)
        .expect_err("header fields are required");
    assert_eq!(
        incomplete_header.category,
        ContractErrorCategory::InvalidShape
    );

    let invalid_json = PersistedSessionRecord::parse("{").expect_err("valid JSON is required");
    assert_eq!(invalid_json.category, ContractErrorCategory::InvalidJson);
}
