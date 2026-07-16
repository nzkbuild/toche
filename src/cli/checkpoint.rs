use anyhow::Context;

use crate::continuity::checkpoint::{CheckpointDb, NewCheckpoint};
use crate::continuity::observer;
use crate::profiles::loader::config_dir;
use crate::safe_cache::workspace;

pub async fn run_save(
    task: Option<String>,
    completed: Option<Vec<String>>,
    next: Option<String>,
    changed_files: Option<Vec<String>>,
    verification: Option<String>,
    open_risks: Option<Vec<String>>,
    model_assisted: bool,
) -> anyhow::Result<()> {
    let project_path = crate::meter::recorder::current_project_path();
    let db_path = config_dir().join("ledger.db");
    let db = CheckpointDb::open(&db_path)
        .with_context(|| format!("Failed to open checkpoint DB at {}", db_path.display()))?;

    // Collect accumulated session facts
    let facts = observer::drain_facts();
    let facts_json = serde_json::to_string(&serde_json::json!({
        "files_read": facts.files_read,
        "files_written": facts.files_written,
        "commands_run": facts.commands_run,
        "models_used": facts.models_used,
    }))
    .unwrap_or_else(|_| "{}".into());

    let entry = NewCheckpoint {
        project_path: project_path.clone(),
        task: task.unwrap_or_default(),
        completed: completed.unwrap_or_default().join("\n"),
        changed_files: changed_files.unwrap_or_default().join("\n"),
        verification: verification.unwrap_or_default(),
        open_risks: open_risks.unwrap_or_default().join("\n"),
        next_action: next.unwrap_or_default(),
        facts_json,
        model_assisted,
    };

    let id = db.insert(&entry).context("Failed to save checkpoint")?;
    println!("Saved checkpoint #{id} for project '{project_path}'.");

    if model_assisted {
        println!("[model_assisted] This checkpoint was generated with model assistance.");
    }

    Ok(())
}

pub async fn run_show(id: Option<i64>, json: bool) -> anyhow::Result<()> {
    let project_path = crate::meter::recorder::current_project_path();
    let db_path = config_dir().join("ledger.db");
    let db = CheckpointDb::open(&db_path)
        .with_context(|| format!("Failed to open checkpoint DB at {}", db_path.display()))?;

    let entry = match id {
        Some(id) => db
            .get(id)?
            .with_context(|| format!("Checkpoint #{id} not found"))?,
        None => db.latest(&project_path)?.context(
            "No checkpoints saved yet for this project. Use 'toche checkpoint save' to create one.",
        )?,
    };

    if json {
        let output = serde_json::json!({
            "id": entry.id,
            "project_path": entry.project_path,
            "git_head": entry.git_head,
            "workspace_fingerprint": entry.workspace_fingerprint,
            "task": entry.task,
            "completed": entry.completed,
            "changed_files": entry.changed_files,
            "verification": entry.verification,
            "open_risks": entry.open_risks,
            "next_action": entry.next_action,
            "facts_json": entry.facts_json,
            "model_assisted": entry.model_assisted,
            "created_at": entry.created_at,
            "updated_at": entry.updated_at,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("Failed to serialize checkpoint")?
        );
        return Ok(());
    }

    // Stale detection
    let current_git = current_git_head();
    let current_ws = workspace::compute_workspace_fingerprint();

    let git_match =
        current_git == entry.git_head || entry.git_head.is_empty() || current_git.is_empty();
    let ws_match = current_ws == entry.workspace_fingerprint;

    println!("Checkpoint #{}", entry.id);
    println!("  Project:        {}", entry.project_path);
    println!("  Created:        {}", entry.created_at);
    println!(
        "  Model-assisted:  {}",
        if entry.model_assisted { "yes" } else { "no" }
    );
    println!();

    println!("Stale detection:");
    if git_match {
        if entry.git_head.is_empty() {
            println!("  Git HEAD:       (no git data)");
        } else {
            println!(
                "  Git HEAD:       MATCH ({})",
                &entry.git_head[..8.min(entry.git_head.len())]
            );
        }
    } else {
        println!("  Git HEAD:       MISMATCH");
        println!(
            "    Cached:   {}",
            &entry.git_head[..8.min(entry.git_head.len())]
        );
        println!("    Current:  {}", &current_git[..8.min(current_git.len())]);
    }
    if ws_match {
        println!("  Workspace:      MATCH");
    } else {
        println!("  Workspace:      MISMATCH — files changed since checkpoint");
    }

    if !git_match || !ws_match {
        println!();
        println!("  WARNING: This checkpoint may be stale. Proceed with caution.");
    }
    println!();

    // Sections
    if !entry.task.is_empty() {
        println!("Task:");
        println!("  {}", entry.task);
        println!();
    }

    if !entry.completed.is_empty() {
        println!("Completed:");
        for line in entry.completed.lines() {
            println!("  - {}", line);
        }
        println!();
    }

    if !entry.changed_files.is_empty() {
        println!("Changed files:");
        for line in entry.changed_files.lines() {
            println!("  {}", line);
        }
        println!();
    }

    if !entry.verification.is_empty() {
        println!("Verification:");
        println!("  {}", entry.verification);
        println!();
    }

    if !entry.open_risks.is_empty() {
        println!("Open risks:");
        for line in entry.open_risks.lines() {
            println!("  - {}", line);
        }
        println!();
    }

    if !entry.next_action.is_empty() {
        println!("Next action:");
        println!("  {}", entry.next_action);
        println!();
    }

    // Facts summary
    if let Ok(facts) = serde_json::from_str::<serde_json::Value>(&entry.facts_json) {
        let has_facts = facts
            .as_object()
            .map(|o| {
                o.values()
                    .any(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            })
            .unwrap_or(false);
        if has_facts {
            println!("Session facts (auto-collected):");
            if let Some(files) = facts.get("files_read").and_then(|v| v.as_array()) {
                if !files.is_empty() {
                    println!("  Files read: {}", files.len());
                }
            }
            if let Some(files) = facts.get("files_written").and_then(|v| v.as_array()) {
                if !files.is_empty() {
                    println!("  Files written: {}", files.len());
                }
            }
            if let Some(cmds) = facts.get("commands_run").and_then(|v| v.as_array()) {
                if !cmds.is_empty() {
                    println!("  Commands run: {}", cmds.len());
                }
            }
        }
    }

    Ok(())
}

pub async fn run_list(json: bool) -> anyhow::Result<()> {
    let project_path = crate::meter::recorder::current_project_path();
    let db_path = config_dir().join("ledger.db");
    let db = CheckpointDb::open(&db_path)
        .with_context(|| format!("Failed to open checkpoint DB at {}", db_path.display()))?;

    let entries = db
        .list(&project_path, 50)
        .context("Failed to list checkpoints")?;

    if json {
        let output: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "task": e.task,
                    "next_action": e.next_action,
                    "model_assisted": e.model_assisted,
                    "created_at": e.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("Failed to serialize checkpoint list")?
        );
    } else {
        if entries.is_empty() {
            println!("No checkpoints saved yet.");
        } else {
            println!("Checkpoints for this project:");
            for e in &entries {
                let task_preview = if e.task.len() > 60 {
                    format!("{}...", &e.task[..57])
                } else if e.task.is_empty() {
                    "(no task)".into()
                } else {
                    e.task.clone()
                };
                let ma = if e.model_assisted { " [model]" } else { "" };
                println!(
                    "  #{}  {:20}  {}{}",
                    e.id,
                    &e.created_at[..19.min(e.created_at.len())],
                    task_preview,
                    ma,
                );
            }
        }
    }

    Ok(())
}

pub async fn run_delete(id: i64) -> anyhow::Result<()> {
    let db_path = config_dir().join("ledger.db");
    let db = CheckpointDb::open(&db_path)
        .with_context(|| format!("Failed to open checkpoint DB at {}", db_path.display()))?;

    match db.delete(id)? {
        true => println!("Deleted checkpoint #{id}."),
        false => anyhow::bail!("Checkpoint #{id} not found."),
    }

    Ok(())
}

fn current_git_head() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}
