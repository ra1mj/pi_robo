use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;
use std::process::Command;

#[test]
fn workspace_dependency_boundaries_are_enforced() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata packages must be an array")?;

    let allowed: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::from([
        ("pi-agent", BTreeSet::from(["pi-model", "pi-protocol"])),
        (
            "pi-cli",
            BTreeSet::from([
                "pi-agent",
                "pi-model",
                "pi-protocol",
                "pi-provider",
                "pi-resources",
                "pi-runtime",
                "pi-store",
                "pi-tools",
            ]),
        ),
        ("pi-model", BTreeSet::from(["pi-protocol"])),
        ("pi-protocol", BTreeSet::new()),
        ("pi-provider", BTreeSet::from(["pi-model", "pi-protocol"])),
        ("pi-resources", BTreeSet::from(["pi-protocol", "pi-store"])),
        (
            "pi-runtime",
            BTreeSet::from([
                "pi-agent",
                "pi-model",
                "pi-protocol",
                "pi-resources",
                "pi-store",
                "pi-tools",
            ]),
        ),
        ("pi-store", BTreeSet::from(["pi-protocol"])),
        (
            "pi-test-support",
            BTreeSet::from(["pi-model", "pi-protocol", "pi-store"]),
        ),
        ("pi-tools", BTreeSet::from(["pi-agent", "pi-protocol"])),
    ]);

    let workspace_packages: Vec<&Value> = packages
        .iter()
        .filter(|package| {
            package["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("pi-"))
        })
        .collect();
    assert_eq!(workspace_packages.len(), allowed.len());

    for package in workspace_packages {
        let name = package["name"]
            .as_str()
            .ok_or("package name must be a string")?;
        let dependencies = package["dependencies"]
            .as_array()
            .ok_or("package dependencies must be an array")?;
        let internal: BTreeSet<&str> = dependencies
            .iter()
            .filter_map(|dependency| dependency["name"].as_str())
            .filter(|dependency| dependency.starts_with("pi-"))
            .collect();
        assert_eq!(
            internal, allowed[name],
            "unexpected dependency edge for {name}"
        );
        assert!(
            name == "pi-test-support" || !internal.contains("pi-test-support"),
            "production crate {name} depends on pi-test-support"
        );
        for dependency in dependencies {
            if dependency["source"].is_string() {
                let requirement = dependency["req"]
                    .as_str()
                    .ok_or("dependency requirement must be a string")?;
                assert!(
                    requirement.starts_with('='),
                    "external dependency {name}/{} is not exactly pinned: {requirement}",
                    dependency["name"].as_str().unwrap_or("<unknown>")
                );
            }
        }
    }
    Ok(())
}
