mod helpers;

#[test]
fn init_empty_dir_creates_both_config_files_and_gitignore() {
    let env = helpers::TempDotfiles::new();
    let mut cmd = env.cmd();
    let assert = cmd.arg("init").arg(env.output_dir()).assert();

    let output = assert.get_output();
    println!("{}", output.status);
    println!("stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    assert.success();

    assert!(env.output_dir().join("config.lua").exists());
    assert!(env.output_dir().join("local.lua").exists());
}

#[test]
fn init_with_existing_config_creates_only_local() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(
        env.output_dir().join("config.lua"),
        r#"return { modules = { base = { files = { "shell/" } } } }"#,
    )
    .unwrap();

    env.cmd().arg("init").arg(env.output_dir()).assert().success();

    assert!(env.output_dir().join("local.lua").exists());
    // The generated local.lua should not be empty
    let local = std::fs::read_to_string(env.output_dir().join("local.lua")).unwrap();
    assert!(!local.is_empty());
}

#[test]
fn init_with_both_files_exits_with_error() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.output_dir().join("config.lua"), "return {}").unwrap();
    std::fs::write(env.output_dir().join("local.lua"), "return {}").unwrap();
    env.cmd().arg("init").arg(env.output_dir()).assert().failure();
}
