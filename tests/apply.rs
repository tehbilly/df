mod helpers;

fn setup_basic(env: &helpers::TempDotfiles) {
    std::fs::create_dir_all(env.repo_dir().join("shell")).unwrap();
    std::fs::write(env.repo_dir().join("shell/aliases"), "alias ll='ls -la'").unwrap();
    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                base = { files = { { src = "shell/aliases", dst = ".config/shell/aliases" } } }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "base" } }"#,
    )
    .unwrap();
}

#[test]
fn apply_creates_symlink_at_expected_destination() {
    let env = helpers::TempDotfiles::new();
    setup_basic(&env);
    env.cmd().arg("apply").assert().success();
    let dst = env.output_dir().join(".config/shell/aliases");
    assert!(dst.is_symlink());
}

#[test]
fn apply_template_uses_local_var_override() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.repo_dir().join("gitconfig"), "email = {{ email }}").unwrap();
    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                git = {
                    files = { { src = "gitconfig", dst = ".gitconfig", type = "template" } },
                    vars  = { email = "global@example.com" },
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "git" }, vars = { email = "work@company.com" } }"#,
    )
    .unwrap();

    env.cmd().arg("apply").assert().success();
    assert_eq!(env.output_file_contents(".gitconfig"), "email = work@company.com");
}

#[test]
fn apply_skips_externally_modified_file_without_force() {
    let env = helpers::TempDotfiles::new();
    setup_basic(&env);
    std::fs::write(
        // Use copy instead of symlink for this test
        env.repo_dir().join("config.lua"),
        r#"return {
          modules = {
              base = { files = { { src = "shell/aliases", dst = ".config/shell/aliases", type = "copy" } } }
          }
      }"#,
    )
    .unwrap();
    // First apply
    env.cmd().arg("apply").assert().success();
    // User edits the deployed file
    let dst = env.output_dir().join(".config/shell/aliases");
    std::fs::write(&dst, "user modification").unwrap();
    // Second apply without --force: should warn but not overwrite
    env.cmd().arg("apply").assert().success();
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "user modification");
}

#[test]
fn apply_force_backs_up_and_overwrites_externally_modified() {
    let env = helpers::TempDotfiles::new();
    setup_basic(&env);
    std::fs::write(
        // Use copy instead of symlink for this test
        env.repo_dir().join("config.lua"),
        r#"return {
          modules = {
              base = { files = { { src = "shell/aliases", dst = ".config/shell/aliases", type = "copy" } } }
          }
      }"#,
    )
    .unwrap();
    env.cmd().arg("apply").assert().success();
    let dst = env.output_dir().join(".config/shell/aliases");
    std::fs::write(&dst, "user modification").unwrap();

    env.cmd().args(["apply", "--force"]).assert().success();

    // Deployed file has the dba-managed content
    let deployed = std::fs::read_to_string(&dst).unwrap();
    assert_eq!(deployed, "alias ll='ls -la'");

    // .backups/ dir contains the user's modification
    let backups = env.repo_dir().join(".backups");
    let backed_up = std::fs::read_dir(&backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join(".config/shell/aliases");
    assert_eq!(std::fs::read_to_string(&backed_up).unwrap(), "user modification");
}

#[test]
fn apply_dep_post_hook_runs_before_dependent_pre_hook() {
    let env = helpers::TempDotfiles::new();
    let log = env.output_dir().join("hook_log.txt");
    let log_str = log.to_str().unwrap();

    std::fs::write(env.repo_dir().join("base_file"), "").unwrap();
    std::fs::write(env.repo_dir().join("dep_file"), "").unwrap();

    std::fs::write(
        env.repo_dir().join("config.lua"),
        format!(
            r#"return {{
            modules = {{
                base = {{
                    files = {{ "base_file" }},
                    hooks = {{ post_apply = "echo base_post >> {}" }},
                }},
                top = {{
                    files = {{ "dep_file" }},
                    deps  = {{ "base" }},
                    hooks = {{ pre_apply = "echo top_pre >> {}" }},
                }},
            }}
        }}"#,
            log_str, log_str
        ),
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "base", "top" } }"#,
    )
    .unwrap();

    env.cmd().arg("apply").assert().success();

    let log_content = std::fs::read_to_string(&log).unwrap();
    let base_post_pos = log_content.find("base_post").unwrap();
    let top_pre_pos = log_content.find("top_pre").unwrap();
    assert!(base_post_pos < top_pre_pos);
}

#[test]
fn apply_force_dep_failure_still_skips_dependent() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.repo_dir().join("a_file"), "").unwrap();
    std::fs::write(env.repo_dir().join("b_file"), "").unwrap();

    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                dep = {
                    files = { "a_file" },
                    hooks = { post_apply = "exit 1" },
                },
                top = {
                    files = { "b_file" },
                    deps  = { "dep" },
                },
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "dep", "top" } }"#,
    )
    .unwrap();

    // --force should not override dep failure propagation
    env.cmd().args(["apply", "--force"]).assert().failure();
    // b_file (from "top" module) must not be deployed
    assert!(!env.output_path_exists("b_file"));
}

#[test]
fn apply_module_flag_does_not_orphan_other_modules() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.repo_dir().join("a"), "").unwrap();
    std::fs::write(env.repo_dir().join("b"), "").unwrap();

    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
            modules = {
                mod_a = { files = { "a" } },
                mod_b = { files = { "b" } },
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "mod_a", "mod_b" } }"#,
    )
    .unwrap();

    // Full apply first
    env.cmd().arg("apply").assert().success();
    // Partial apply — only mod_a
    env.cmd().args(["apply", "--module", "mod_a"]).assert().success();

    // mod_b should not appear as orphaned in status output
    let output = env.cmd().arg("status").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone())
        // For better validation
        .unwrap()
        .to_lowercase();
    println!("{}", stdout);
    assert!(stdout.contains("mod_a"));
    assert!(stdout.contains("mod_b"));
    assert!(!stdout.contains("orphan"));
}

#[test]
fn apply_module_flag_only_deploys_named_module() {
    let env = helpers::TempDotfiles::new();
    std::fs::write(env.repo_dir().join("a"), "content_a").unwrap();
    std::fs::write(env.repo_dir().join("b"), "content_b").unwrap();

    std::fs::write(
        env.repo_dir().join("config.lua"),
        r#"return {
              modules = {
                  mod_a = { files = { { src = "a", dst = "a", type = "copy" } } },
                  mod_b = { files = { { src = "b", dst = "b", type = "copy" } } },
              }
          }"#,
    )
    .unwrap();
    std::fs::write(
        env.repo_dir().join("local.lua"),
        r#"return { modules = { "mod_a", "mod_b" } }"#,
    )
    .unwrap();

    env.cmd().args(["apply", "--module", "mod_a"]).assert().success();

    // mod_a's file is deployed
    assert!(env.output_path_exists("a"), "mod_a should be deployed");
    // mod_b's file is not deployed
    assert!(!env.output_path_exists("b"), "mod_b should not be deployed");
}
