pub mod kafka_handler;
mod utils;

use kafka_handler::KafkaReader;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::{AsyncReadExt, BufReader};
use tokio_postgres::NoTls;

use data_transporter::db::{db_connection_config_from_env, DBHandler};
#[allow(dead_code)]
#[derive(Deserialize)]
struct ToolConfig {
    name: String, //name
    binary_path: String,
    run: Vec<String>, // how to run
}

#[derive(Deserialize)]
struct Config {
    tools: Vec<ToolConfig>,
}
#[allow(unused_variables)]
#[allow(clippy::needless_borrows_for_generic_args)]
#[allow(clippy::let_unit_value)]
/// Input: a message with version
/// output: a file
pub async fn analyse_once(
    kafka_reader: &KafkaReader,
    output_path: &str,
) -> Result<(), Box<dyn Error>> {
    let config_path = Path::new("/var/tools/tools.json");
    let config: Config =
        serde_json::from_str(&fs::read_to_string(config_path)?).expect("Failed to parse config");

    let tools = config.tools;

    let message = kafka_reader.read_single_message().await.unwrap();
    tracing::info!("Analysis receive {:?}", message);
    tracing::info!(
        "name:{},git_url:{:?}",
        message.db_model.crate_name,
        message.db_model.mega_url
    );
    let namespace = utils::extract_namespace(&message.db_model.mega_url).await?;

    tracing::info!("analyze namespace:{}", namespace.clone());

    let repo_path = PathBuf::from("/var/target/new_crates_file").join(&namespace);
    tracing::info!("code_path:{:?}", repo_path.clone());

    for tool in &tools {
        for command in &tool.run {
            let output_file = PathBuf::from(output_path)
                .join(&tool.name)
                .join(&namespace)
                .join(message.db_model.crate_name.clone() + ".txt");
            let output_dir = PathBuf::from(output_path).join(&tool.name).join(&namespace);

            tracing::info!("output_file_path:{:?}", output_file.clone());
            tracing::info!("output_dir:{:?}", output_dir.clone());
            if !output_dir.is_dir() {
                let _ = tokio::fs::create_dir_all(&output_dir).await;
            }
            let f = tokio::fs::File::create(&output_file).await.unwrap();

            let gitleaks = PathBuf::from("/var/tools/sensleak/gitleaks.toml");
            let mut cmd = Command::new("/var/tools/sensleak/scan");
            cmd.args(&[
                "--repo",
                repo_path.to_str().unwrap(),
                "--config",
                gitleaks.to_str().unwrap(),
                "-v",
                "--pretty",
                "--report",
                output_file.to_str().unwrap(),
            ]);
            let output = cmd.output()?;
            tracing::info!("output:{:?}", output);
            if !output.status.success() {
                let error_msg = String::from_utf8_lossy(&output.stderr);
                tracing::info!("Command failed with error: {}", error_msg);
                return Err(format!(
                    "Failed to execute run command for {}: {}",
                    tool.name, error_msg
                )
                .into());
            }
            tracing::info!("finish command");
            //insert into pg
            let db_connection_config = db_connection_config_from_env();
            #[allow(unused_variables)]
            let (client, connection) = tokio_postgres::connect(&db_connection_config, NoTls)
                .await
                .unwrap();
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("connection error: {}", e);
                }
            });
            let dbhandler = DBHandler { client };
            let id = namespace.clone();
            let file = tokio::fs::File::open(output_file).await?;
            let mut reader = BufReader::new(file);
            let mut content = String::new();
            reader.read_to_string(&mut content).await?;
            tracing::info!("content:{}", content.clone());
            let _ = dbhandler
                .insert_sensleak_result_into_pg(id.clone(), content.clone())
                .await
                .unwrap();
        }
    }

    Ok(())
}
#[allow(dead_code)]
fn init_git(repo_path: &str) -> Result<(), ()> {
    if let Err(e) = std::env::set_current_dir(Path::new(repo_path)) {
        println!("Failed to change directory: {}", e);
    } else {
        let init_output = Command::new("git")
            .arg("init")
            .output()
            .expect("Failed to execute git init");
        if !init_output.status.success() {
            let error_msg = String::from_utf8_lossy(&init_output.stderr);
            println!("git init failed: {}", error_msg);
        }
        let add_output = Command::new("git")
            .arg("add")
            .arg(".")
            .output()
            .expect("Failed to execute git add");
        if !add_output.status.success() {
            let error_msg = String::from_utf8_lossy(&add_output.stderr);
            println!("git add failed: {}", error_msg);
        }
        let commit_output = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("first commit")
            .output()
            .expect("Failed to execute git commit");
        if !commit_output.status.success() {
            let error_msg = String::from_utf8_lossy(&commit_output.stderr);
            println!("git commit failed: {}", error_msg);
        }
    }
    Ok(())
}
