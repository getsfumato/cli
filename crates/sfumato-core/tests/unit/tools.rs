//! Unit tests for the generation-tool gates.

use crate::config::{
    Capability, EffectiveConfig, GenerationToolDefaults, GenerationToolKind, GlobalConfig,
    PageDefaults, ProjectSecurityConfig,
};
use std::path::PathBuf;

use super::chart_tool_gate_warning;

fn config_with(chart_enabled: bool, allow_python: bool) -> EffectiveConfig {
    let global = GlobalConfig::default_config();
    let mut tools = GenerationToolDefaults::default();
    tools.0.insert(GenerationToolKind::ChartGen, chart_enabled);
    let security = ProjectSecurityConfig {
        allow_python,
        ..ProjectSecurityConfig::default()
    };
    EffectiveConfig {
        user: global.user,
        project_name: "university".to_string(),
        project_root: PathBuf::from("/tmp/university"),
        publish_dir: None,
        theme: "sfumato-default".to_string(),
        connectors: global.connectors,
        models: global.models,
        model_defaults: global.defaults.0,
        model_roles: global.model_roles,
        page: PageDefaults::default(),
        generation_tools: tools,
        security,
        knowledge: Default::default(),
        marp: global.marp,
    }
}

#[test]
fn a_per_run_approval_permits_the_charting_tool() {
    // The defect: `enable` read only the persisted `security.allow_python`, so a
    // caller who consented for this run with `--allow-code-execution` got the tool
    // withheld — and no error, because a tool that is never offered cannot fail.
    let config = config_with(true, false);
    assert!(super::python_permitted(&config, true));
    assert!(chart_tool_gate_warning(&config, true).is_none());
}

#[test]
fn a_blocked_charting_tool_is_explained_rather_than_dropped_in_silence() {
    let config = config_with(true, false);
    let warning = chart_tool_gate_warning(&config, false)
        .expect("an enabled tool the gate withheld has to say so");
    assert!(warning.contains("chart-gen"));
    assert!(
        warning.contains("--allow-code-execution") && warning.contains("allow_python"),
        "the warning has to name both ways out: {warning}"
    );
}

#[test]
fn a_persisted_trust_decision_still_permits_the_tool() {
    let config = config_with(true, true);
    assert!(super::python_permitted(&config, false));
    assert!(chart_tool_gate_warning(&config, false).is_none());
}

#[test]
fn a_tool_nobody_enabled_produces_no_warning() {
    // Silence is right here: the project did not ask for charts, so there is
    // nothing to explain and a warning on every run would be noise.
    let config = config_with(false, false);
    assert!(chart_tool_gate_warning(&config, false).is_none());
    assert!(chart_tool_gate_warning(&config, true).is_none());
}

#[test]
fn image_capability_defaults_are_unaffected_by_the_chart_gate() {
    // Guards the shared `generation_tool_enabled` path the gate reads through.
    let config = config_with(true, true);
    assert!(!config.model_defaults.contains_key(&Capability::Image));
}
