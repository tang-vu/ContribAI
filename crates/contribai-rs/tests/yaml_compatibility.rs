//! Regression coverage for the maintained YAML parser's application contract.

use contribai::core::admission::{ExecutionMode, RepositoryConsent, CONSENT_PATHS};
use contribai::core::config::{ContribAIConfig, LlmConfig};
use contribai::core::error::ContribError;
use contribai::core::profiles::load_profile_yaml;
use contribai::templates::TemplateRegistry;

#[test]
fn consent_rejects_ambiguous_or_malformed_yaml() {
    for yaml in [
        "enabled: false\nenabled: true\n",
        "schema_version: 1\nschema_version: 2\nenabled: true\n",
        "enabled: true\n---\nenabled: false\n",
        "enabled: true\nallowed_paths: [src/**\n",
        "enabled: true\nallowed_paths: *missing\n",
        "enabled: 'true'\n",
        "enabled: yes\n",
        "enabled: true\nunknown_permission: true\n",
    ] {
        assert!(
            RepositoryConsent::parse(CONSENT_PATHS[0], yaml).is_none(),
            "ambiguous or invalid consent must fail closed: {yaml:?}"
        );
    }
}

#[test]
fn consent_preserves_anchors_and_restrictive_schema_two_defaults() {
    let consent = RepositoryConsent::parse(
        CONSENT_PATHS[0],
        "schema_version: 2\nenabled: true\nallowed_paths: &paths [src/**]\ndenied_paths: *paths\n",
    )
    .unwrap();

    assert_eq!(consent.schema_version, 2);
    assert_eq!(consent.allowed_paths, ["src/**"]);
    assert_eq!(consent.denied_paths, ["src/**"]);
    assert!(!consent.allow_dependency_changes);
    assert!(consent.draft_only);
    assert_eq!(consent.execution_mode, ExecutionMode::Local);
}

#[test]
fn configuration_round_trip_keeps_safe_defaults() {
    let config: ContribAIConfig = serde_yaml::from_str("{}").unwrap();
    let yaml = serde_yaml::to_string(&config).unwrap();
    let decoded: ContribAIConfig = serde_yaml::from_str(&yaml).unwrap();

    assert!(decoded.pipeline.dry_run);
    assert_eq!(decoded.pipeline.agent_mode, "plan");
    assert!(!decoded.scheduler.enabled);
    assert!(decoded.sandbox.require_validation);
}

#[test]
fn profiles_and_templates_preserve_yaml_scalars_and_defaults() {
    let profile = load_profile_yaml("name: 'security: review'\ndry_run: true\n").unwrap();
    assert_eq!(profile.name, "security: review");
    assert!(profile.dry_run);
    assert_eq!(profile.severity_threshold, "medium");

    let mut templates = TemplateRegistry::new();
    templates.load_yaml(
        "name: yaml-compatibility\ndescription: 'Review: safely'\ntype: code_quality\npattern: example\nfix_template: |\n  first line\n  second line\nlanguages: [rust]\n",
    );
    let template = templates.get("yaml-compatibility").unwrap();
    assert_eq!(template.description, "Review: safely");
    assert_eq!(template.fix_template, "first line\nsecond line\n");
    assert_eq!(template.severity, "medium");
    assert_eq!(template.languages, ["rust"]);
}

#[test]
fn parser_errors_still_convert_to_application_errors() {
    let error = serde_yaml::from_str::<ContribAIConfig>("pipeline: [").unwrap_err();
    let error: ContribError = error.into();
    assert!(matches!(error, ContribError::Yaml(_)));
}

#[test]
fn provider_config_preserves_the_value_conversion_api() {
    #[derive(serde::Deserialize)]
    struct ProviderConfig {
        llm: serde_yaml::Value,
    }

    let config: ProviderConfig =
        serde_yaml::from_str("llm:\n  provider: ollama\n  model: local-test\n").unwrap();
    assert_eq!(
        config
            .llm
            .get("provider")
            .and_then(serde_yaml::Value::as_str),
        Some("ollama")
    );
    let llm: LlmConfig = serde_yaml::from_value(config.llm).unwrap();
    assert_eq!(llm.provider, "ollama");
    assert_eq!(llm.model, "local-test");
    assert!(llm.api_key.is_empty());
    assert!(!llm.use_vertex());
    assert!(!llm.copilot);
}
