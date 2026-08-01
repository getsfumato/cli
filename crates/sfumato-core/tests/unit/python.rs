use super::*;

#[test]
fn screening_rejects_escapes_out_of_the_run_directory() {
    for source in [
        "import os\nprint(os.environ)",
        "import subprocess",
        "data = open('/etc/passwd').read()",
        "__import__('socket')",
        "eval('1+1')",
    ] {
        assert!(
            screen_python_source(source).is_err(),
            "expected {source:?} to be refused"
        );
    }
}

#[test]
fn screening_accepts_ordinary_plotting_code() {
    let source = "import numpy as np\nimport matplotlib.pyplot as plt\nplt.plot(np.arange(10))";
    screen_python_source(source).expect("plotting code should pass the screen");
}

#[test]
fn screening_is_case_insensitive() {
    assert!(screen_python_source("import OS").is_err());
}

#[test]
fn requirements_accept_plain_and_pinned_names() {
    for requirement in [
        "numpy",
        "matplotlib==3.10.7",
        "scikit-learn",
        "manim==0.20.1",
    ] {
        validate_requirement(requirement)
            .unwrap_or_else(|error| panic!("{requirement} should be allowed: {error}"));
    }
}

#[test]
fn requirements_reject_flags_urls_and_paths() {
    for requirement in [
        "",
        "--index-url=http://evil",
        "-e .",
        "numpy @ https://example.com/numpy.whl",
        "./local-package",
        "numpy==",
        "numpy==v1",
    ] {
        assert!(
            validate_requirement(requirement).is_err(),
            "expected {requirement:?} to be refused"
        );
    }
}

#[test]
fn run_paths_stay_inside_the_run_directory() {
    validate_run_path("chart.png").expect("a plain name is inside the run directory");
    validate_run_path("scenes/intro.py").expect("a nested name is inside the run directory");
    for path in [
        "/etc/passwd",
        "../escape.png",
        "scenes/../../escape.py",
        "  ",
    ] {
        assert!(
            validate_run_path(path).is_err(),
            "expected {path:?} to be refused"
        );
    }
}
