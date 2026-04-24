use anyhow::Result;
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser)]
#[command(name = "vibe", about = "Vibe2D game engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Vibe2D game project
    New {
        /// Project name
        name: String,
    },
    /// Query the running game via VDP
    Inspect {
        /// VDP server address
        #[arg(short, long, default_value = "ws://127.0.0.1:9229")]
        addr: String,
    },
    /// Send a JSON-RPC command to the running game
    Rpc {
        /// JSON-RPC method name
        method: String,
        /// JSON params (optional)
        #[arg(default_value = "{}")]
        params: String,
        /// VDP server address
        #[arg(short, long, default_value = "ws://127.0.0.1:9229")]
        addr: String,
    },
    /// Take a screenshot of the running game
    Screenshot {
        /// Output file path
        #[arg(short, long, default_value = "screenshot.png")]
        output: String,
        /// VDP server address
        #[arg(short, long, default_value = "ws://127.0.0.1:9229")]
        addr: String,
    },
    /// Show engine version info
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { name } => {
            println!("Creating new Vibe2D project: {}", name);
            create_project(&name)?;
        }
        Commands::Inspect { addr } => {
            let result = vdp_call(&addr, "game.inspect", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Rpc {
            method,
            params,
            addr,
        } => {
            // Try to parse the user-supplied `params` as JSON; fall back to
            // wrapping the raw string so `vibe rpc foo hello` still works.
            // `unwrap_or` is fine here because the fallback value is already
            // owned — no allocation to defer.
            let params: serde_json::Value =
                serde_json::from_str(&params).unwrap_or(serde_json::Value::String(params));
            let result = vdp_call(&addr, &method, params).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Screenshot { output, addr } => {
            let result = vdp_call(
                &addr,
                "game.screenshot",
                serde_json::json!({ "path": output }),
            )
            .await?;
            if let Some(err) = result.get("error") {
                eprintln!("Screenshot failed: {}", err);
            } else {
                // Wait briefly for the screenshot to be written on the next frame
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                println!("Screenshot saved to: {}", output);
            }
        }
        Commands::Version => {
            println!("vibe2d {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}

async fn vdp_call(
    addr: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(addr).await?;
    let (mut write, mut read) = ws_stream.split();

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    write
        .send(Message::Text(serde_json::to_string(&request)?.into()))
        .await?;

    if let Some(Ok(Message::Text(text))) = read.next().await {
        let response: serde_json::Value = serde_json::from_str(&text)?;
        Ok(response)
    } else {
        anyhow::bail!("No response from VDP server")
    }
}

fn create_project(name: &str) -> Result<()> {
    let project_dir = std::path::Path::new(name);
    if project_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    std::fs::create_dir_all(project_dir.join("src"))?;
    std::fs::create_dir_all(project_dir.join("assets"))?;

    // Cargo.toml
    std::fs::write(
        project_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
vibe2d = {{ git = "https://github.com/user/vibe2d" }}
rand = "0.9"
serde_json = "1"
"#
        ),
    )?;

    // game.yaml
    std::fs::write(
        project_dir.join("game.yaml"),
        format!(
            r#"meta:
  name: "{name}"
  version: "0.1.0"

window:
  width: 1280
  height: 720
  title: "{name}"
  vsync: true

virtual_resolution:
  width: 320
  height: 180

input:
  actions:
    jump:
      keys: ["Space"]

debug:
  vdp:
    enabled: true
    port: 9229
"#
        ),
    )?;

    // src/main.rs
    std::fs::write(
        project_dir.join("src/main.rs"),
        r#"use vibe2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn new(_ctx: &mut Context) -> Self {
        Self
    }

    fn update(&mut self, _ctx: &mut Context, _dt: f32, _input: &InputState) {}

    fn draw(&mut self, _ctx: &Context, _screen: &mut Screen) {}
}

fn main() {
    vibe2d::run::<MyGame>("game.yaml");
}
"#,
    )?;

    println!(
        "Created project '{}'. Run with: cd {} && cargo run",
        name, name
    );
    Ok(())
}
