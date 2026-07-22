mod support;

use pi_agent::Tool;
use pi_tools::{
    BashTool, BashToolConfig, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, EditTool, ImagePolicy,
    MutationCoordinator, ReadTool, WriteTool,
};
use serde_json::Value;
use support::TempRoot;

#[test]
fn schemas_and_limits_match_the_captured_typescript_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/tools/contracts.json"))
            .expect("tool contract fixture");
    assert_eq!(fixture["limits"]["lines"], DEFAULT_MAX_LINES);
    assert_eq!(fixture["limits"]["bytes"], DEFAULT_MAX_BYTES);

    let root = TempRoot::new("typescript-contract");
    let mutations = MutationCoordinator::default();
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadTool::new(root.path(), ImagePolicy::default())),
        Box::new(BashTool::new(BashToolConfig::new(root.path()))),
        Box::new(EditTool::new(root.path(), mutations.clone())),
        Box::new(WriteTool::new(root.path(), mutations)),
    ];
    for tool in tools {
        let definition = tool.definition();
        let captured = &fixture["schemas"][&definition.name];
        let required = definition.parameters["required"]
            .as_array()
            .expect("required array");
        for name in captured["required"].as_array().expect("captured required") {
            assert!(required.contains(name), "missing required field {name}");
        }
        assert_eq!(definition.parameters["additionalProperties"], false);
    }
}
