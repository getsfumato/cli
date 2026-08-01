use super::*;

#[test]
fn bundled_manifest_declares_the_environments_workflows_ask_for() {
    let manifest = UvPythonRuntime::parse_manifest().expect("bundled manifest should parse");
    assert_eq!(manifest.schema_version, 1);
    for expected in ["charting", "manim"] {
        let entry = manifest
            .environments
            .get(expected)
            .unwrap_or_else(|| panic!("manifest should declare the '{expected}' environment"));
        assert!(!entry.packages.is_empty());
        // A floating pin would make two runs of the same generated chart produce
        // different pictures, which is the guarantee this manifest exists to keep.
        for package in &entry.packages {
            assert!(
                package.contains("=="),
                "{expected} requirement '{package}' is not pinned"
            );
        }
    }
}

#[test]
fn a_base_environment_resolves_to_its_own_layer() {
    let resolved = UvPythonRuntime::resolve("charting", &[]).expect("charting resolves");
    assert_eq!(resolved.layer, "charting");
    assert_eq!(resolved.packages, resolved.spec.packages);
}

#[test]
fn extra_packages_land_in_a_derived_layer_that_keeps_the_base_intact() {
    let resolved =
        UvPythonRuntime::resolve("charting", &["scipy==1.16.2".to_string()]).expect("resolves");
    assert!(resolved.layer.starts_with("charting-"));
    assert_ne!(resolved.layer, "charting");
    assert!(resolved.packages.contains(&"scipy==1.16.2".to_string()));
    for pinned in &resolved.spec.packages {
        assert!(resolved.packages.contains(pinned));
    }
}

#[test]
fn extra_package_order_does_not_build_two_identical_layers() {
    let forward = UvPythonRuntime::resolve(
        "charting",
        &["scipy==1.16.2".to_string(), "pandas==2.3.3".to_string()],
    )
    .expect("resolves");
    let reversed = UvPythonRuntime::resolve(
        "charting",
        &["pandas==2.3.3".to_string(), "scipy==1.16.2".to_string()],
    )
    .expect("resolves");
    assert_eq!(forward.layer, reversed.layer);
    assert_eq!(forward.requirements(), reversed.requirements());
}

#[test]
fn resolution_refuses_an_unknown_environment_and_an_unsafe_requirement() {
    assert!(UvPythonRuntime::resolve("nope", &[]).is_err());
    assert!(
        UvPythonRuntime::resolve("charting", &["--index-url=http://evil".to_string()]).is_err()
    );
}

#[test]
fn a_layer_is_stale_until_its_recorded_requirements_match() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = UvPythonRuntime::new(root.path().to_path_buf());
    let resolved = UvPythonRuntime::resolve("charting", &[]).expect("resolves");
    assert!(!runtime.is_current(&resolved), "nothing is installed yet");

    let interpreter = runtime.interpreter(&resolved.layer);
    std::fs::create_dir_all(interpreter.parent().expect("interpreter parent")).expect("mkdir");
    std::fs::write(&interpreter, "").expect("interpreter placeholder");
    assert!(
        !runtime.is_current(&resolved),
        "an interpreter with no recorded pins is not trustworthy"
    );

    std::fs::write(runtime.stamp(&resolved.layer), "matplotlib==0.0.1").expect("stale stamp");
    assert!(
        !runtime.is_current(&resolved),
        "recorded pins that differ from the manifest are stale"
    );

    std::fs::write(runtime.stamp(&resolved.layer), resolved.requirements()).expect("stamp");
    assert!(runtime.is_current(&resolved));
}

#[test]
fn removing_a_base_environment_also_removes_layers_derived_from_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = UvPythonRuntime::new(root.path().to_path_buf());
    for layer in ["charting", "charting-abc123", "manim"] {
        std::fs::create_dir_all(root.path().join(layer)).expect("layer directory");
    }
    runtime.remove("charting").expect("charting is removable");
    assert!(!root.path().join("charting").exists());
    assert!(
        !root.path().join("charting-abc123").exists(),
        "a derived layer would keep the removed pins installed under another name"
    );
    assert!(root.path().join("manim").exists());
}

#[test]
fn removing_an_unknown_environment_is_refused() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = UvPythonRuntime::new(root.path().to_path_buf());
    assert!(runtime.remove("nope").is_err());
}

#[test]
fn failure_reporting_keeps_the_tail_where_python_puts_the_reason() {
    let noisy = (0..80)
        .map(|index| format!("warning {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let stderr = format!("{noisy}\nValueError: x and y must have same first dimension");
    let message = failure_tail("", &stderr);
    assert!(message.contains("ValueError: x and y must have same first dimension"));
    assert!(message.contains("earlier line(s) omitted"));
    assert!(!message.contains("warning 0"));
}

#[test]
fn failure_reporting_falls_back_to_stdout_when_nothing_was_written_to_stderr() {
    let message = failure_tail("the script printed this instead", "   \n");
    assert!(message.contains("the script printed this instead"));
}

#[cfg(feature = "real-renderers")]
#[tokio::test]
async fn a_run_harvests_declared_outputs_and_leaves_no_generated_code_behind() {
    use sfumato_core::python::PythonRunRequest;

    let root = tempfile::tempdir().expect("temporary root");
    let outputs = tempfile::tempdir().expect("temporary outputs");
    let runtime = UvPythonRuntime::new(root.path().to_path_buf());
    let operation = OperationContext::detached();

    let mut files = BTreeMap::new();
    files.insert(
        "main.py".to_string(),
        "import matplotlib\nmatplotlib.use('Agg')\nimport matplotlib.pyplot as plt\n\
         plt.plot([0, 1], [0, 1])\nplt.savefig('chart.png')\n"
            .to_string(),
    );
    let result = runtime
        .run(
            PythonRunRequest {
                environment: "charting".to_string(),
                extra_packages: Vec::new(),
                files,
                entrypoint: "main.py".to_string(),
                arguments: Vec::new(),
                outputs: vec!["chart.png".to_string()],
                output_dir: outputs.path().to_path_buf(),
            },
            &operation,
        )
        .await
        .expect("the chart should render");

    assert_eq!(result.outputs.len(), 1);
    assert!(result.outputs[0].is_file());
    assert!(
        !outputs.path().join("main.py").exists(),
        "generated code must never be harvested alongside the output"
    );
}
