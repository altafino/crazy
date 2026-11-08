//! The real ArtCraft test (eng T9, R7). Opt-in, because it starts the installed ArtCraft apps:
//!
//! `STUDIO_CRAFTS_E2E=1 cargo test real_crafts_test -- --nocapture`
//!
//! Without the variable it returns at once. Every wired app found on this machine is started with
//! the argv `Crafts::for_run` generates, speaks MCP over stdio (initialize, tools/list), and
//! PhotoCraft must export a PNG under the project's `assets/`. The project lives under `$HOME`
//! because Flatpak apps cannot see the host's `/tmp`.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use super::*;
use crate::domain::{CraftsEntry, ProjectMeta, TemplateId};

const STEP: Duration = Duration::from_secs(60);

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    out: tokio::io::Lines<BufReader<ChildStdout>>,
    next: u64,
}

impl Mcp {
    fn start(spec: &McpServerSpec) -> Self {
        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|e| panic!("{}: spawn {}: {e}", spec.name, spec.command));
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            stdin,
            out,
            next: 0,
        }
    }

    async fn send(&mut self, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    /// One request; skips notifications and anything that is not our reply. Returns `result`,
    /// panics with the server's error otherwise.
    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        timeout(STEP, async {
            loop {
                let line = self
                    .out
                    .next_line()
                    .await
                    .unwrap()
                    .unwrap_or_else(|| panic!("server closed during {method}"));
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if v["id"] == json!(id) && v.get("method").is_none() {
                    if let Some(e) = v.get("error") {
                        panic!("{method} failed: {e}");
                    }
                    return v["result"].clone();
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{method} timed out"))
    }

    async fn tool(&mut self, name: &str, args: Value) -> Value {
        self.call("tools/call", json!({"name": name, "arguments": args}))
            .await
    }

    async fn handshake(&mut self) -> (String, Vec<Value>) {
        let init = self
            .call(
                "initialize",
                json!({"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "studio-real-crafts-test", "version": "0"}}),
            )
            .await;
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        let version = init["serverInfo"]["version"]
            .as_str()
            .unwrap_or("?")
            .to_owned();
        let tools = self.call("tools/list", json!({})).await["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        (version, tools)
    }

    async fn stop(mut self) {
        drop(self.stdin);
        let _ = self.child.kill().await;
    }
}

/// Every file under `root` except `assets/`, relative, sorted.
fn outside_assets(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if rel == Path::new(ASSETS_DIR) {
                continue;
            }
            out.push(rel);
            if p.is_dir() {
                walk(&p, root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn pngs(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return found;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            found.extend(pngs(&p));
        } else if p.extension().is_some_and(|x| x == "png") {
            found.push(p);
        }
    }
    found
}

#[tokio::test]
async fn real_crafts_test() {
    if std::env::var("STUDIO_CRAFTS_E2E").as_deref() != Ok("1") {
        eprintln!("real_crafts_test: skipped (set STUDIO_CRAFTS_E2E=1)");
        return;
    }
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let cache = home.join(".cache");
    std::fs::create_dir_all(&cache).unwrap();
    let tmp = tempfile::tempdir_in(&cache).unwrap();
    let project = tmp.path().join("proj");
    std::fs::create_dir_all(project.join(".ai-app-studio")).unwrap();

    let env: Vec<(String, String)> = std::env::vars().collect();
    let path = std::env::var("PATH").unwrap_or_default();
    let detection = detect(&env, &path, Some(&home)).await;
    let apps: Vec<String> = detection
        .apps
        .iter()
        .filter(|a| a.spec.mcp.is_some())
        .map(|a| a.spec.id.to_owned())
        .collect();
    println!("detected wired apps: {apps:?}");
    if apps.is_empty() {
        println!("no ArtCraft apps installed; nothing to test");
        return;
    }

    let mut meta = ProjectMeta::new(
        "crafts-e2e",
        "crafts-e2e",
        TemplateId::TauriVue,
        "crafts-e2e",
    );
    meta.crafts = Some(CraftsEntry { apps: apps.clone() });
    meta.write(&project).unwrap();

    // An attachments folder too, so every confined app gets two --automation-read-root.
    let attachments = tmp.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    let plan = Crafts::new(tmp.path().join("extract"), Some(home))
        .for_run(&project, Some(&attachments), &env)
        .await;
    println!("warnings: {:?}", plan.warnings);
    assert!(!plan.servers.is_empty(), "no servers planned for {apps:?}");
    let before = outside_assets(&project);

    for spec in &plan.servers {
        println!("starting {}: {} {}", spec.name, spec.command, spec.args.join(" "));
        let mut mcp = Mcp::start(spec);
        let (version, tools) = mcp.handshake().await;
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        println!("{}: version {version}, {} tools", spec.name, names.len());
        assert!(!names.is_empty(), "{} lists no tools", spec.name);
        if spec.name == "craft_photocraft" {
            photocraft_export(&mut mcp, &project).await;
        }
        mcp.stop().await;
    }

    assert_eq!(
        outside_assets(&project),
        before,
        "an engine wrote outside assets/"
    );
}

/// A 32x32 coin through `doc_new` + `doc_save` into `assets/`; the file must be a real PNG.
async fn photocraft_export(mcp: &mut Mcp, project: &Path) {
    let assets = project.join(ASSETS_DIR);
    let target = assets.join("coin.png");
    for (tool, args) in [
        (
            "doc_new",
            json!({"width": 32, "height": 32, "background": "#ffcc00", "name": "coin"}),
        ),
        ("doc_save", json!({"path": target})),
        ("doc_close", json!({})),
    ] {
        let r = mcp.tool(tool, args).await;
        println!(
            "  {tool}: {}",
            r["content"][0]["text"].as_str().unwrap_or("")
        );
        assert_ne!(r["isError"], json!(true), "{tool} failed: {r}");
    }
    let found = pngs(&assets);
    println!("  PNG written: {found:?}");
    let bytes = std::fs::read(&target).expect("coin.png was not written under assets/");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
}
