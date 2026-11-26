use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn closed_stdout_exits_quietly() {
    let dir = std::env::temp_dir().join(format!("scli-sigpipe-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("scli")).unwrap();
    let servers: serde_json::Map<String, serde_json::Value> = (0..20_000)
        .map(|i| {
            (
                format!("workspace-{i:05}"),
                serde_json::json!({"token": "xoxp-x"}),
            )
        })
        .collect();
    std::fs::write(
        dir.join("scli").join("config.json"),
        serde_json::json!({ "servers": servers }).to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_scli"))
        .args(["read", "workspaces"])
        .env("XDG_CONFIG_HOME", &dir)
        .env("SCLI_NO_UPDATE_CHECK", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 16];
    stdout.read_exact(&mut first).unwrap();
    drop(stdout);
    let out = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).ok();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(out.status.code(), Some(0));
}
