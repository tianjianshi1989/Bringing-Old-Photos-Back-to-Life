use tauri::Emitter;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModifyPhotoArgs {
    run_id: String,
    input_path: String,
    output_folder: Option<String>,
    gpu: String,
    with_scratch: bool,
    hr: bool,
    python: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModifyPhotoResult {
    output_path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    run_id: String,
    stage: Option<u8>,
    message: String,
    is_error: bool,
}

fn stage_from_line(line: &str) -> Option<u8> {
    if line.contains("Running Stage 1") {
        return Some(1);
    }
    if line.contains("Running Stage 2") {
        return Some(2);
    }
    if line.contains("Running Stage 3") {
        return Some(3);
    }
    if line.contains("Running Stage 4") {
        return Some(4);
    }
    None
}

fn emit_progress(app: &tauri::AppHandle, event: ProgressEvent) {
    let _ = app.emit("modify_progress", event);
}

fn project_root() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui_dir = manifest_dir
        .parent()
        .ok_or_else(|| "Unable to locate ui directory".to_string())?;
    let root_dir = ui_dir
        .parent()
        .ok_or_else(|| "Unable to locate project root directory".to_string())?;
    Ok(root_dir.to_path_buf())
}

fn ensure_single_image_folder(input_path: &Path, output_folder: &Path) -> Result<PathBuf, String> {
    let input_dir = output_folder.join("_gui_input");
    if input_dir.exists() {
        fs::remove_dir_all(&input_dir).map_err(|e| format!("Failed to clear _gui_input: {e}"))?;
    }
    fs::create_dir_all(&input_dir).map_err(|e| format!("Failed to create _gui_input: {e}"))?;

    let file_name = input_path
        .file_name()
        .ok_or_else(|| "Invalid input file path".to_string())?;
    let dst = input_dir.join(file_name);
    fs::copy(input_path, &dst).map_err(|e| format!("Failed to copy input file: {e}"))?;
    Ok(input_dir)
}

fn pick_latest_file(dir_path: &Path) -> Result<Option<PathBuf>, String> {
    if !dir_path.is_dir() {
        return Ok(None);
    }

    let mut latest: Option<(SystemTime, PathBuf)> = None;
    let entries = fs::read_dir(dir_path).map_err(|e| format!("Failed to read dir: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {e}"))?;
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.starts_with('.') {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .map_err(|e| format!("Failed to stat file {name}: {e}"))?;

        match &latest {
            None => latest = Some((modified, p)),
            Some((cur, _)) => {
                if modified > *cur {
                    latest = Some((modified, p));
                }
            }
        }
    }

    Ok(latest.map(|(_, p)| p))
}

#[tauri::command]
async fn modify_photo(app: tauri::AppHandle, args: ModifyPhotoArgs) -> Result<ModifyPhotoResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let app = app;
        let run_id = args.run_id.clone();

        emit_progress(
            &app,
            ProgressEvent {
                run_id: run_id.clone(),
                stage: Some(0),
                message: "Starting...".to_string(),
                is_error: false,
            },
        );

        let root = project_root()?;

        let output_folder = match args.output_folder {
            Some(of) if !of.trim().is_empty() => {
                let p = PathBuf::from(of);
                if p.is_absolute() {
                    p
                } else {
                    root.join(p)
                }
            }
            _ => root.join("output_gui"),
        };
        fs::create_dir_all(&output_folder)
            .map_err(|e| format!("Failed to create output folder: {e}"))?;

        let input_path = PathBuf::from(args.input_path);
        if !input_path.exists() {
            return Err(format!("Input not found: {}", input_path.display()));
        }

        let input_folder = if input_path.is_dir() {
            input_path
        } else {
            ensure_single_image_folder(&input_path, &output_folder)?
        };

        let stage_dirs = [
            output_folder.join("stage_1_restore_output"),
            output_folder.join("stage_2_detection_output"),
            output_folder.join("stage_3_face_output"),
            output_folder.join("final_output"),
        ];
        for dir in stage_dirs {
            if dir.exists() {
                fs::remove_dir_all(&dir)
                    .map_err(|e| format!("Failed to clear {}: {e}", dir.display()))?;
            }
            fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
        }

        let final_dir = output_folder.join("final_output");

        let run_py = root.join("run.py");
        if !run_py.exists() {
            return Err(format!("run.py not found: {}", run_py.display()));
        }

        let mut cmd = Command::new(&args.python);
        cmd.current_dir(&root);
        cmd.env("PYTHONUNBUFFERED", "1");
        cmd.arg("-u");
        cmd.arg(run_py);
        cmd.arg("--input_folder").arg(input_folder);
        cmd.arg("--output_folder").arg(&output_folder);
        cmd.arg("--GPU").arg(args.gpu);
        if args.with_scratch {
            cmd.arg("--with_scratch");
        }
        if args.hr {
            cmd.arg("--HR");
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to start python: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture python stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Failed to capture python stderr".to_string())?;

        let (tx, rx) = mpsc::channel::<(bool, String)>();
        let tx_out = tx.clone();
        let tx_err = tx.clone();

        let out_handle = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                let _ = tx_out.send((false, line));
            }
        });
        let err_handle = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                let _ = tx_err.send((true, line));
            }
        });
        drop(tx);

        let mut stage: Option<u8> = Some(0);
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok((is_error, line)) => {
                    if let Some(s) = stage_from_line(&line) {
                        stage = Some(s);
                    }
                    emit_progress(
                        &app,
                        ProgressEvent {
                            run_id: run_id.clone(),
                            stage,
                            message: line,
                            is_error,
                        },
                    );
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) =
                        child.try_wait().map_err(|e| format!("Process error: {e}"))?
                    {
                        let _ = out_handle.join();
                        let _ = err_handle.join();
                        if !status.success() {
                            emit_progress(
                                &app,
                                ProgressEvent {
                                    run_id: run_id.clone(),
                                    stage,
                                    message: format!("Python exited with status: {status}"),
                                    is_error: true,
                                },
                            );
                            return Err(format!("Python exited with status: {status}"));
                        }
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let latest = pick_latest_file(&final_dir)?
            .ok_or_else(|| format!("No output image found under {}", final_dir.display()))?;

        emit_progress(
            &app,
            ProgressEvent {
                run_id: run_id.clone(),
                stage: Some(4),
                message: "Done".to_string(),
                is_error: false,
            },
        );

        Ok(ModifyPhotoResult {
            output_path: latest.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, modify_photo])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
