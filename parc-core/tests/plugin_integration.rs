#![cfg(feature = "wasm-plugins")]

use std::path::PathBuf;

use parc_core::config::Config;
use parc_core::plugin;
use parc_core::plugin::manager::PluginManager;
use parc_core::plugin::runtime::WasmRuntime;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("plugins")
        .join("echo-plugin")
}

fn setup_vault_with_plugin() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    // Copy plugin files into vault
    let plugins_dir = vault.join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let fixture_dir = fixtures_dir();
    std::fs::copy(fixture_dir.join("echo.wasm"), plugins_dir.join("echo.wasm")).unwrap();
    std::fs::copy(fixture_dir.join("echo.toml"), plugins_dir.join("echo.toml")).unwrap();

    (tmp, vault)
}

#[test]
fn test_load_manifest() {
    let manifest_path = fixtures_dir().join("echo.toml");
    let manifest = plugin::load_manifest(&manifest_path).unwrap();
    assert_eq!(manifest.plugin.name, "echo");
    assert_eq!(manifest.plugin.version, "0.1.0");
    assert!(manifest.capabilities.read_fragments);
    assert!(!manifest.capabilities.write_fragments);
    assert!(manifest.capabilities.allows_hook("post-create"));
    assert!(!manifest.capabilities.allows_hook("pre-create"));
}

#[test]
fn test_discover_plugins_in_vault() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let discovered = plugin::discover_plugins(&vault).unwrap();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest.plugin.name, "echo");
}

#[test]
fn test_validate_manifest_ok() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let discovered = plugin::discover_plugins(&vault).unwrap();
    assert!(plugin::validate_manifest(&discovered[0].manifest, &vault).is_ok());
}

#[test]
fn test_load_and_init_plugin() {
    let fixture_dir = fixtures_dir();
    let wasm_bytes = std::fs::read(fixture_dir.join("echo.wasm")).unwrap();
    let manifest = plugin::load_manifest(&fixture_dir.join("echo.toml")).unwrap();

    let runtime = WasmRuntime::new().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let instance = runtime.load_plugin(manifest, &wasm_bytes, &vault, "{}");
    assert!(instance.is_ok());
}

#[test]
fn test_plugin_event_handler() {
    let fixture_dir = fixtures_dir();
    let wasm_bytes = std::fs::read(fixture_dir.join("echo.wasm")).unwrap();
    let manifest = plugin::load_manifest(&fixture_dir.join("echo.toml")).unwrap();

    let runtime = WasmRuntime::new().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let mut instance = runtime
        .load_plugin(manifest, &wasm_bytes, &vault, "{}")
        .unwrap();

    let result = instance
        .call_event("post-create", r#"{"id":"test"}"#)
        .unwrap();
    assert!(result.is_some());
    let output = result.unwrap();
    assert!(output.contains("event=post-create"));
    assert!(output.contains("fragment="));
}

#[test]
fn test_plugin_command_handler() {
    let fixture_dir = fixtures_dir();
    let wasm_bytes = std::fs::read(fixture_dir.join("echo.wasm")).unwrap();
    let manifest = plugin::load_manifest(&fixture_dir.join("echo.toml")).unwrap();

    let runtime = WasmRuntime::new().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let mut instance = runtime
        .load_plugin(manifest, &wasm_bytes, &vault, "{}")
        .unwrap();

    let output = instance
        .call_command("echo", r#"["hello","world"]"#)
        .unwrap();
    assert!(output.contains("cmd=echo"));
    assert!(output.contains("args="));
}

#[test]
fn test_plugin_manager_load_all() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let config = Config::default();
    let manager = PluginManager::load_all(&vault, &config).unwrap();
    assert_eq!(manager.plugins.len(), 1);
}

#[test]
fn test_plugin_manager_list_commands() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let config = Config::default();
    let manager = PluginManager::load_all(&vault, &config).unwrap();
    let commands = manager.list_commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].command, "echo");
    assert_eq!(commands[0].plugin_name, "echo");
}

#[test]
fn test_plugin_manager_execute_command() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let config = Config::default();
    let mut manager = PluginManager::load_all(&vault, &config).unwrap();
    let output = manager
        .execute_command("echo", "echo", &["hello".into()])
        .unwrap();
    assert!(output.contains("cmd=echo"));
}

#[test]
fn test_plugin_manager_dispatch_post_event() {
    let (_tmp, vault) = setup_vault_with_plugin();
    let config = Config::default();
    let mut manager = PluginManager::load_all(&vault, &config).unwrap();

    let fragment = parc_core::fragment::Fragment {
        id: "TEST123".into(),
        fragment_type: "note".into(),
        title: "Test".into(),
        tags: vec![],
        links: vec![],
        attachments: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        created_by: None,
        extra_fields: std::collections::BTreeMap::new(),
        body: "test body".into(),
    };

    // Should not panic — post events don't return errors
    manager.dispatch_post_event(parc_core::hook::HookEvent::PostCreate, &fragment);
}

#[test]
fn test_plugin_install_remove() {
    let (_tmp, vault) = setup_vault_with_plugin();

    // Verify plugin exists
    let discovered = plugin::discover_plugins(&vault).unwrap();
    assert_eq!(discovered.len(), 1);

    // Remove plugin files
    let plugins_dir = vault.join("plugins");
    std::fs::remove_file(plugins_dir.join("echo.wasm")).unwrap();
    std::fs::remove_file(plugins_dir.join("echo.toml")).unwrap();

    // Verify plugin is gone
    let discovered = plugin::discover_plugins(&vault).unwrap();
    assert_eq!(discovered.len(), 0);
}

#[test]
fn test_capability_hook_filtering() {
    let manifest = plugin::PluginManifest {
        plugin: plugin::PluginMeta {
            name: "test".into(),
            version: "0.1.0".into(),
            description: "".into(),
            wasm: "test.wasm".into(),
        },
        capabilities: plugin::PluginCapabilities {
            hooks: vec!["post-create".into()],
            render: vec!["note".into()],
            validate: vec!["*".into()],
            ..Default::default()
        },
    };

    assert!(manifest.capabilities.allows_hook("post-create"));
    assert!(!manifest.capabilities.allows_hook("pre-create"));
    assert!(!manifest.capabilities.allows_hook("post-update"));

    assert!(manifest.capabilities.allows_render("note"));
    assert!(!manifest.capabilities.allows_render("todo"));

    assert!(manifest.capabilities.allows_validate("note"));
    assert!(manifest.capabilities.allows_validate("todo"));
    assert!(manifest.capabilities.allows_validate("anything"));
}

#[test]
fn test_empty_plugin_manager() {
    let manager = PluginManager::empty().unwrap();
    assert!(manager.plugins.is_empty());
    assert!(manager.list_commands().is_empty());
}

#[test]
fn test_doctor_plugin_checks() {
    let (_tmp, vault) = setup_vault_with_plugin();

    // Healthy vault with valid plugin
    let findings = parc_core::doctor::check_plugins(&vault);
    assert!(
        findings.is_empty(),
        "expected no findings, got: {:?}",
        findings
    );

    // Break the plugin by removing the wasm
    std::fs::remove_file(vault.join("plugins").join("echo.wasm")).unwrap();
    let findings = parc_core::doctor::check_plugins(&vault);
    assert_eq!(findings.len(), 1);
    assert!(matches!(
        &findings[0],
        parc_core::doctor::DoctorFinding::PluginIssue { plugin_name, .. } if plugin_name == "echo"
    ));
}
