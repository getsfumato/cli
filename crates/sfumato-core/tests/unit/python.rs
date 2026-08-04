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
            screen_python_source(source, &[]).is_err(),
            "expected {source:?} to be refused"
        );
    }
}

#[test]
fn screening_accepts_ordinary_plotting_code() {
    let source = "import numpy as np\nimport matplotlib.pyplot as plt\nplt.plot(np.arange(10))";
    screen_python_source(source, &[]).expect("plotting code should pass the screen");
}

#[test]
fn screening_is_case_insensitive() {
    assert!(screen_python_source("import OS", &[]).is_err());
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

#[test]
fn screening_refuses_paraphrases_of_the_imports_it_blocks() {
    // Every one of these passed the substring denylist. `from os import path` is
    // not a sophisticated bypass — it is the same import spelled the other way.
    for source in [
        "from os import path",
        "from pathlib import Path",
        "from pathlib import Path\nPath('/etc/hosts').read_text()",
        "import importlib",
        "import pty",
        "import os.path",
        "import numpy, os",
        "import shutil as s",
        "from subprocess import run",
        "import ctypes",
        "import pickle",
    ] {
        assert!(
            screen_python_source(source, &[]).is_err(),
            "expected {source:?} to be refused"
        );
    }
}

#[test]
fn screening_still_accepts_what_chart_and_scene_code_needs() {
    for source in [
        "import matplotlib.pyplot as plt\nplt.plot([1, 2])",
        "from matplotlib import pyplot as plt",
        "import numpy as np\nfrom numpy import linspace",
        "import math\nimport json\nfrom collections import Counter",
        "from sympy import symbols, solve",
        "from manim import Scene, Circle",
        "from mpl_toolkits.mplot3d import Axes3D",
        "import random\nimport statistics\nimport itertools",
    ] {
        screen_python_source(source, &[])
            .unwrap_or_else(|error| panic!("expected {source:?} to pass the screen: {error}"));
    }
}

#[test]
fn a_project_authorised_package_may_be_imported() {
    // Without this, a project that layers its own dependency through
    // `security.python_packages` could not import what it installed.
    let allowed = vec!["pandas==2.2.3".to_string()];

    screen_python_source("import pandas as pd", &allowed).expect("an authorised package imports");
    // A hyphenated distribution name imports with an underscore.
    let dashed = vec!["scikit-image".to_string()];
    screen_python_source("import scikit_image", &dashed).expect("the module spelling is accepted");
    // Authorising one package does not open the allowlist generally.
    assert!(screen_python_source("import os", &allowed).is_err());
}

#[test]
fn the_refusal_names_the_module_and_what_is_allowed() {
    let error = screen_python_source("from pathlib import Path", &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("pathlib"), "{error}");
    assert!(error.contains("matplotlib"), "{error}");
}

#[test]
fn a_dangerous_call_is_still_refused_without_an_import() {
    for source in ["breakpoint()", "eval('1')", "exec('x=1')", "open('f')"] {
        assert!(
            screen_python_source(source, &[]).is_err(),
            "expected {source:?} to be refused"
        );
    }
}
