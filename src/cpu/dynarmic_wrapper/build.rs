/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use std::env;
use std::path::Path;

fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
}
fn link_search(path: &Path) {
    println!("cargo:rustc-link-search=native={}", path.to_str().unwrap());
}
fn link_lib(lib: &str) {
    println!("cargo:rustc-link-lib=static={lib}");
}

fn build_type_windows() -> &'static str {
    let os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS was not set");
    if os.eq_ignore_ascii_case("windows") {
        if cfg!(debug_assertions) { "Debug" } else { "Release" }
    } else {
        ""
    }
}

fn main() {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = package_root.join("../../..");

    let mut build = cmake::Config::new(workspace_root.join("vendor/dynarmic"));
    let frontends = if env::var_os("CARGO_FEATURE_EXPERIMENTAL_AARCH64").is_some() {
        "A32;A64"
    } else {
        "A32"
    };
    build.define("DYNARMIC_FRONTENDS", frontends);
    build.define("DYNARMIC_WARNINGS_AS_ERRORS", "OFF");
    build.define("DYNARMIC_TESTS", "OFF");
    build.define("DYNARMIC_USE_BUNDLED_EXTERNALS", "ON");
    build.define("CMAKE_POLICY_VERSION_MINIMUM", "3.5");
    build.cxxflag("-DFMT_CONSTEVAL=");

    let os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS was not set");
    let boost_path = workspace_root.join("vendor/boost");
    if (os.eq_ignore_ascii_case("windows") || os.eq_ignore_ascii_case("android"))
        && !boost_path.is_dir()
    {
        panic!("Could not find Boost. Download it from https://www.boost.org/users/download/ and put it at vendor/boost");
    }
    if boost_path.is_dir() {
        build.define("Boost_INCLUDE_DIR", boost_path);
    }
    if os.eq_ignore_ascii_case("android") {
        build.define("CMAKE_SYSTEM_NAME", "Android");
        build.define("CMAKE_SYSTEM_VERSION", "21");
        build.define("ANDROID", "ON");
        build.define("CMAKE_ANDROID_ARCH_ABI", "arm64-v8a");
    }
    let dynarmic_out = build.build();

    if os.eq_ignore_ascii_case("android") {
        let mut cc_command = cc::Build::new().get_compiler().to_command();
        let libclang_rt_path = cc_command
            .arg("-print-libgcc-file-name")
            .output()
            .unwrap()
            .stdout;
        let libclang_rt_path: &Path = std::str::from_utf8(&libclang_rt_path).unwrap().as_ref();
        link_search(libclang_rt_path.parent().unwrap());
        link_lib(
            libclang_rt_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .trim()
                .strip_prefix("lib")
                .unwrap()
                .strip_suffix(".a")
                .unwrap(),
        );
    }

    link_search(&dynarmic_out.join("lib"));
    link_search(&dynarmic_out.join("lib64"));
    link_lib("dynarmic");
    link_search(&dynarmic_out.join("build/externals/fmt").join(build_type_windows()));
    link_lib(if cfg!(debug_assertions) { "fmtd" } else { "fmt" });
    link_search(&dynarmic_out.join("build/externals/mcl/src").join(build_type_windows()));
    link_lib("mcl");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH was not set");
    if arch.eq_ignore_ascii_case("x86_64") {
        link_search(&dynarmic_out.join("build/externals/zydis").join(build_type_windows()));
        link_lib("Zydis");
    }

    let mut wrapper_build = cc::Build::new();
    wrapper_build
        .file(package_root.join("lib.cpp"))
        .cpp(true)
        .std("c++17")
        .include(dynarmic_out.join("include"));
    if !cfg!(debug_assertions) {
        wrapper_build.define("NDEBUG", "1");
    }
    wrapper_build.compile("dynarmic_wrapper");
    rerun_if_changed(&package_root.join("lib.cpp"));
}
