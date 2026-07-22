use pi_test_support::{CompatibilityState, fixture_path, validate_compatibility_catalog};
use serde_json::json;
use std::error::Error;
use std::fs;

#[test]
fn compatibility_catalog_has_valid_verified_evidence() -> Result<(), Box<dyn Error>> {
    let path = fixture_path("rust/fixtures/compatibility.json")?;
    let catalog = validate_compatibility_catalog(&path).map_err(|errors| errors.join("\n"))?;
    assert!(catalog.entries.len() >= 9);
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| entry.state == CompatibilityState::Verified)
    );
    Ok(())
}

#[test]
fn invalid_compatibility_evidence_is_rejected() -> Result<(), Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "pi-compatibility-invalid-{}.json",
        std::process::id()
    ));
    let entry = json!({
        "id": "duplicate",
        "milestone": "M1",
        "area": "protocol",
        "owner": "unknown-owner",
        "oracle": "",
        "fixture": "rust/fixtures/missing.json",
        "runner": "rust/tests/missing.rs",
        "normalizers": ["/"],
        "state": "verified"
    });
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "entries": [entry.clone(), entry]
        }))?,
    )?;
    let errors = validate_compatibility_catalog(&path).expect_err("invalid catalog must fail");
    fs::remove_file(path)?;
    let joined = errors.join("\n");
    assert!(joined.contains("duplicate compatibility id"));
    assert!(joined.contains("unknown owner"));
    assert!(joined.contains("missing oracle"));
    assert!(joined.contains("missing fixture path"));
    assert!(joined.contains("missing runner path"));
    assert!(joined.contains("non-root JSON pointer"));
    Ok(())
}
