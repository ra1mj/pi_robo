use pi_cli::{ModelServiceFactory, ProductionModelServiceFactory};
use pi_model::ModelServiceErrorCategory;
use pi_protocol::{Extensions, Model, ModelCost, ModelInput, Settings};
use pi_store::{CredentialSource, ResolvedCredential, SecretString};
use std::collections::BTreeMap;

fn model(api: &str) -> Model {
    Model {
        id: "fixture".to_owned(),
        name: "Fixture".to_owned(),
        api: api.to_owned(),
        provider: "fixture".to_owned(),
        base_url: "http://127.0.0.1:1/v1".to_owned(),
        reasoning: false,
        input: vec![ModelInput::Text],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window: 128_000,
        max_tokens: 4_096,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn credential() -> ResolvedCredential {
    ResolvedCredential {
        secret: SecretString::new("synthetic"),
        source: CredentialSource::CliOverride,
        environment: BTreeMap::new(),
    }
}

#[test]
fn production_registry_contains_exactly_the_four_milestone_protocols() {
    let factory = ProductionModelServiceFactory;
    let credential = credential();
    for api in [
        "openai-completions",
        "openai-responses",
        "anthropic-messages",
        "google-generative-ai",
    ] {
        factory
            .create(&model(api), Some(&credential), &Settings::default())
            .unwrap_or_else(|error| panic!("{api}: {error}"));
    }
    let error = match factory.create(&model("faux"), Some(&credential), &Settings::default()) {
        Ok(_) => panic!("production Faux registration must not exist"),
        Err(error) => error,
    };
    assert_eq!(error.category, ModelServiceErrorCategory::Configuration);
}
