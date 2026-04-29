mod helpers;

#[test]
fn diff_shows_unified_diff_for_modified_template() {
    let env = helpers::TempDotfiles::new();

    std::fs::write(env.repo_dir().join("tmux.conf"), "theme = {{ theme }}").unwrap();
    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                tmux = {
                    files = { { src = "tmux.conf", dst = ".tmux.conf", type = "template" } },
                    vars  = { theme = "default" },
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(env.repo_dir().join("local.lua"), r#"return { modules = { "tmux" } }"#).unwrap();

    env.cmd().arg("apply").assert().success();

    // Change the deployed file to simulate a drift
    std::fs::write(env.output_dir().join(".tmux.conf"), "theme = modified").unwrap();

    let output = env.cmd().arg("diff").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    // A unified diff must contain these markers
    println!("{}", stdout);
    assert!(stdout.contains("---") || stdout.contains("+++") || stdout.contains("@@"));
}
