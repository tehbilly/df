mod helpers;

#[test]
fn init_empty_dir_creates_both_config_files_and_gitignore() {
    let env = helpers::TempDotfiles::new();
    env.cmd().arg("init").arg(env.output_dir.path()).assert().success();

    assert!(env.output_dir.path().join("config.lua").exists());
    assert!(env.output_dir.path().join("local.lua").exists());

    let gitignore = std::fs::read_to_string(env.output_dir.path().join(".gitignore")).unwrap();
    println!("{}", gitignore);
    assert!(gitignore.contains("/local.lua"));
    assert!(gitignore.contains("/.backups/"));
}

#[test]
fn init_with_existing_config_creates_only_local() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(
        env.output_dir.path().join("config.lua"),
        r#"return { modules = { base = { files = { "shell/" } } } }"#,
    )
    .unwrap();

    env.cmd().arg("init").arg(env.output_dir.path()).assert().success();

    assert!(env.output_dir.path().join("local.lua").exists());
    // The generated local.lua should mention "base" (from config.lua)
    let local = std::fs::read_to_string(env.output_dir.path().join("local.lua")).unwrap();
    println!("-- local.lua:");
    println!("{}", local);
    assert!(local.contains("base"));
}

#[test]
fn init_with_both_files_exits_with_error() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.output_dir.path().join("config.lua"), "return {}").unwrap();
    std::fs::write(env.output_dir.path().join("local.lua"), "return {}").unwrap();
    env.cmd().arg("init").arg(env.output_dir.path()).assert().failure();
}
