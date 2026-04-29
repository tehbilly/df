mod helpers;

#[test]
fn list_shows_active_and_inactive_modules() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                base   = { files = { "shell/" } },
                neovim = { files = { "nvim/" } },
            }
        }"#,
    )
    .unwrap();
    std::fs::write(env.repo_dir().join("local.lua"), r#"return { modules = { "base" } }"#).unwrap();

    let output = env.cmd().arg("list").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("base"));
    assert!(stdout.contains("neovim"));
    // "base" is active; "neovim" is not
    // The exact format is your choice — verify both names appear and the
    // active status is distinguishable in the output.
}
