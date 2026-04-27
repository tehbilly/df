mod helpers;

fn setup_and_apply(env: &helpers::TempDotfiles) {
    std::fs::write(env.repo_dir.path().join("gitconfig"), "[user]\n  name = Test").unwrap();
    std::fs::write(
        env.repo_dir.path().join("config.lua"),
        r#"return {
            modules = {
                git = { files = { { src = "gitconfig", dst = ".gitconfig", type = "copy" } } }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir.path().join("local.lua"),
        r#"return { modules = { "git" } }"#,
    )
    .unwrap();
    env.cmd().arg("apply").assert().success();
}

#[test]
fn status_shows_clean_after_fresh_deploy() {
    let env = helpers::TempDotfiles::new();
    setup_and_apply(&env);
    let output = env.cmd().arg("status").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(stdout.to_lowercase().contains("up to date"));
}

#[test]
fn status_shows_modified_after_external_change() {
    let env = helpers::TempDotfiles::new();
    setup_and_apply(&env);
    // Modify the deployed file
    std::fs::write(env.output_dir.path().join(".gitconfig"), "modified").unwrap();

    let output = env.cmd().arg("status").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    // The status output should reflect the external modification
    assert!(stdout.to_lowercase().contains("modified") || stdout.to_lowercase().contains("external"));
}
