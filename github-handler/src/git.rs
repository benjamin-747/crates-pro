use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, Utc};
use std::collections::HashSet;
use tokio::process::Command as TokioCommand;
use tracing::{debug, error, info, warn};

pub async fn clone_repo(
    target_dir: &Path,
    owner: &str,
    repo: &str,
    partial_clone: bool,
) -> Result<(), anyhow::Error> {
    debug!("克隆仓库到指定目录: {}", target_dir.display());
    let clone_url = format!("https://github.com/{}/{}.git", owner, repo);
    let path = target_dir.to_string_lossy();
    let mut args = vec![
        "clone",
        "--no-checkout",
        "--config",
        "credential.helper=reject", // 拒绝认证请求，不会提示输入
        "--config",
        "http.lowSpeedLimit=1000", // 设置低速限制
        "--config",
        "http.lowSpeedTime=10", // 如果速度低于限制持续10秒则失败
        "--config",
        "core.askpass=echo", // 不使用交互式密码提示
        &clone_url,
        &path,
    ];
    if partial_clone {
        args.push("--filter=blob:none");
    }
    let status = TokioCommand::new("git").args(args).status().await;

    match status {
        Ok(status) if !status.success() => {
            error!("克隆仓库失败: {}，可能需要认证或不存在，跳过此仓库", status);
            return Err(anyhow::Error::msg("clone failed"));
        }
        Err(e) => {
            error!("执行git命令失败: {}，跳过此仓库", e);
            return Err(e.into());
        }
        _ => {}
    }
    Ok(())
}

pub fn is_shallow_repo(path: &PathBuf) -> bool {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(path)
        .output()
        .expect("Failed to run git");

    String::from_utf8_lossy(&output.stdout).trim() == "true"
}

pub async fn update_repo(
    target_dir: &PathBuf,
    owner: &str,
    repo: &str,
) -> Result<(), anyhow::Error> {
    info!("更新之前clone的仓库: {}", target_dir.display());

    TokioCommand::new("git")
        .current_dir(target_dir)
        .args(["reset", "--hard", "HEAD"])
        .status()
        .await?;

    TokioCommand::new("git")
        .current_dir(target_dir)
        .args(["clean", "-fd"])
        .status()
        .await?;

    let args = vec![
        "-c",
        "credential.helper=reject",
        "-c",
        "http.lowSpeedLimit=1000",
        "-c",
        "http.lowSpeedTime=10",
        "-c",
        "core.askpass=echo",
        "pull",
        "--rebase",
    ];
    let status = TokioCommand::new("git")
        .current_dir(target_dir)
        .args(args)
        .status()
        .await;
    match status {
        Ok(status) => {
            if !status.success() {
                eprintln!("Git command failed with status: {:?}", status);
                std::fs::remove_dir_all(target_dir)?;
                clone_repo(target_dir, owner, repo, false).await?;
            }
        }
        Err(e) => {
            eprintln!("Error executing git command: {}", e);
            return Err(e.into());
        }
    }
    Ok(())
}

pub async fn restore_shallow_repo(target_dir: &PathBuf) -> Result<(), anyhow::Error> {
    info!("恢复clone的shallow仓库: {}", target_dir.display());
    let output = TokioCommand::new("git")
        .current_dir(target_dir)
        .args(["remote", "show", "origin"])
        .env("LANG", "en_US.UTF-8") // 设置输出语言为英文
        .output()
        .await
        .ok()
        .unwrap();

    let stdout = std::str::from_utf8(&output.stdout).ok().unwrap();

    for line in stdout.lines() {
        if line.trim_start().starts_with("HEAD branch:") {
            let default_branch = line
                .split(':')
                .nth(1)
                .map(|s| s.trim().to_string())
                .unwrap();
            let args = vec![
                "-c",
                "credential.helper=reject",
                "-c",
                "http.lowSpeedLimit=1000",
                "-c",
                "http.lowSpeedTime=10",
                "-c",
                "core.askpass=echo",
                "checkout",
                &default_branch,
            ];
            let status = TokioCommand::new("git")
                .current_dir(target_dir)
                .args(args)
                .status()
                .await;
            if let Err(e) = status {
                warn!("更新仓库失败: {}，可能需要认证，继续分析当前代码", e);
            }
        }
    }

    Ok(())
}

fn recent_months(n: usize) -> Vec<(i32, u32)> {
    let now = Utc::now().naive_utc();
    (0..n)
        .map(|i| {
            let d = now - Duration::days(30 * i as i64);
            (d.year(), d.month())
        })
        .collect()
}

fn extract_months_from_timestamps(timestamps: &[i64]) -> HashSet<(i32, u32)> {
    timestamps
        .iter()
        .filter_map(|ts| DateTime::from_timestamp(*ts, 0))
        .map(|dt| (dt.year(), dt.month()))
        .collect()
}

fn all_months_present(target_months: &[(i32, u32)], seen_months: &HashSet<(i32, u32)>) -> bool {
    target_months.iter().all(|m| seen_months.contains(m))
}

pub async fn check_commit_last_three_month(target_dir: &PathBuf) -> bool {
    let target_months = recent_months(3);

    let output = TokioCommand::new("git")
        .current_dir(target_dir)
        .args(["log", "--since=3.months", "--pretty=format:%ct"])
        .output()
        .await
        .expect("failed to execute git log");

    if !output.status.success() {
        eprintln!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let timestamps: Vec<i64> = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i64>().ok())
        .collect();

    let seen_months = extract_months_from_timestamps(&timestamps);
    all_months_present(&target_months, &seen_months)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_extract_months_from_timestamps() {
        let ts1 = Utc
            .with_ymd_and_hms(2025, 7, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let ts2 = Utc
            .with_ymd_and_hms(2025, 6, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let ts3 = Utc
            .with_ymd_and_hms(2025, 5, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let timestamps = vec![ts1, ts2, ts3];

        let months = extract_months_from_timestamps(&timestamps);

        assert!(months.contains(&(2025, 7)));
        assert!(months.contains(&(2025, 6)));
        assert!(months.contains(&(2025, 5)));
        assert_eq!(months.len(), 3);
    }

    #[test]
    fn test_all_months_present_true() {
        let target = vec![(2025, 5), (2025, 6), (2025, 7)];
        let seen = HashSet::from([(2025, 5), (2025, 6), (2025, 7)]);
        assert!(all_months_present(&target, &seen));
    }

    #[test]
    fn test_all_months_present_false() {
        let target = vec![(2025, 5), (2025, 6), (2025, 7)];
        let seen = HashSet::from([(2025, 6), (2025, 7)]);
        assert!(!all_months_present(&target, &seen));
    }
}
