mod helpers;

fn setup_with_orphan(env: &helpers::TempDotfiles) {
    // Deploy two modules, then re-apply with only one active
    std::fs::write(env.repo_dir.path().join("a_file"), "a").unwrap();
    std::fs::write(env.repo_dir.path().join("b_file"), "b").unwrap();
    std::fs::write(
        env.repo_dir.path().join("config.lua"),
        r#"return {
            modules = {
                mod_a = { files = { { src = "a_file", dst = ".a" } } },
                mod_b = { files = { { src = "b_file", dst = ".b" } } },
            }
        }"#,
    )
    .unwrap();
    // First apply: both modules active
    std::fs::write(
        env.repo_dir.path().join("local.lua"),
        r#"return { modules = { "mod_a", "mod_b" } }"#,
    )
    .unwrap();
    env.cmd().arg("apply").assert().success();

    // Second apply: only mod_a active — mod_b becomes orphaned
    std::fs::write(
        env.repo_dir.path().join("local.lua"),
        r#"return { modules = { "mod_a" } }"#,
    )
    .unwrap();
    env.cmd().arg("apply").assert().success();
}

#[test]
fn clean_yes_removes_orphaned_files() {
    let env = helpers::TempDotfiles::new();
    setup_with_orphan(&env);

    assert!(env.output_path_exists(".b"));
    env.cmd().args(["clean", "--yes"]).assert().success();
    assert!(!env.output_path_exists(".b"));
}

#[test]
fn clean_without_yes_non_tty_skips_removal() {
    let env = helpers::TempDotfiles::new();
    setup_with_orphan(&env);

    // Send "n" to stdin
    env.cmd().arg("clean").assert().success();

    assert!(env.output_path_exists(".b"));
}
