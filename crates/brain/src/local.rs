//! Local tool execution: the seven manifest tools against a per-session workspace directory
//! on the operator's own machine.
//!
//! This is the zero-setup default mode. Be clear about what it is NOT: a subprocess is
//! process-level separation, **not a sandbox** -- `bash` here runs arbitrary commands as the
//! operator. Fine for developing against your own brain with your own key; untrusted prompts
//! belong in a sandboxed Hand adapter. The startup banner repeats this.
//!
//! Semantics mirror the hand guest where it matters to the model: identical input schemas
//! (they ARE the sealed manifest schemas), the same output conventions (stdout + `[stderr]`
//! section for bash, text + `[meta]` JSON for the file tools), bounded tail-retained output,
//! cancel with kill. Guest paths map onto the workspace: `/workspace/...` and
//! `/home/agent/...` both resolve into the session's directory; other absolute paths are
//! refused (hygiene, not security).

use crate::adapter::{
    ArtifactMeta, CallOutcome, CallRequest, HandAdapter, HandFactory, HandSpec, LostReport,
    OutputSink, SeedFile, ToolBundleFile, WorkspaceFile, WorkspaceListing,
};
use crate::{BrainError, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

/// Per-stream capture bound: tail-retained beyond this, like the guest's spill.
const MAX_STREAM_BYTES: usize = 1024 * 1024;
/// Default and ceiling for bash's `timeout_ms`.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

/// One session's local workspace.
pub struct LocalHand {
    pub session_id: String,
    root: PathBuf,
    artifacts: PathBuf,
    bundles: PathBuf,
}

impl LocalHand {
    /// Opens (creating if needed) the session's directories under `data_dir`.
    pub fn open(data_dir: &Path, session_id: &str) -> Result<Arc<Self>> {
        let base = data_dir.join(session_id);
        let root = base.join("workspace");
        let artifacts = base.join("artifacts");
        let bundles = base.join("bundles");
        std::fs::create_dir_all(&root)
            .and_then(|()| std::fs::create_dir_all(&artifacts))
            .and_then(|()| std::fs::create_dir_all(&bundles))
            .map_err(|e| BrainError::HandUnavailable(format!("workspace dir: {e}")))?;
        Ok(Arc::new(Self {
            session_id: session_id.to_string(),
            root,
            artifacts,
            bundles,
        }))
    }

    pub fn workspace(&self) -> &Path {
        &self.root
    }

    /// Removes the session's directories (delete).
    pub fn purge(data_dir: &Path, session_id: &str) {
        let base = data_dir.join(session_id);
        if base.exists()
            && let Err(e) = std::fs::remove_dir_all(&base)
        {
            tracing::warn!(session = session_id, error = %e, "workspace purge incomplete");
        }
    }

    /// Writes a seed file into the workspace (create-time `files`).
    pub fn seed(&self, path: &str, bytes: &[u8], _mode: Option<i64>) -> Result<()> {
        let target = self.resolve(path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BrainError::Invalid(format!("seed {path}: {e}")))?;
        }
        std::fs::write(&target, bytes).map_err(|e| BrainError::Invalid(format!("seed {path}: {e}")))
    }

    /// Copies a workspace file into the artifacts directory.
    pub fn persist_file(&self, name: &str, path: &str) -> Result<(u64, String, PathBuf)> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(BrainError::Invalid(
                "artifact name must be a plain filename".into(),
            ));
        }
        let source = self.resolve(path)?;
        let bytes = std::fs::read(&source)
            .map_err(|e| BrainError::Hand(format!("persist read {path}: {e}")))?;
        let target = self.artifacts.join(name);
        std::fs::write(&target, &bytes)
            .map_err(|e| BrainError::Hand(format!("persist write: {e}")))?;
        Ok((
            bytes.len() as u64,
            hex::encode(Sha256::digest(&bytes)),
            target,
        ))
    }

    /// Maps a guest-style path into the workspace. `/workspace/...` and `/home/agent/...`
    /// resolve here; bare relative paths resolve here; anything else absolute is refused, and
    /// `..` may never escape the root.
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let p = path.trim();
        let rel = if let Some(r) = p.strip_prefix("/workspace") {
            r.trim_start_matches('/')
        } else if let Some(r) = p.strip_prefix("/home/agent") {
            r.trim_start_matches('/')
        } else if Path::new(p).is_absolute() || p.starts_with('/') {
            return Err(BrainError::Invalid(format!(
                "path {p} is outside the workspace (use /workspace/... or a relative path)"
            )));
        } else {
            p
        };
        let mut out = self.root.clone();
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(seg) => out.push(seg),
                Component::CurDir => {}
                _ => {
                    return Err(BrainError::Invalid(format!(
                        "path {p} escapes the workspace"
                    )));
                }
            }
        }
        Ok(out)
    }

    fn rel_of(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn api_path(&self, path: &str) -> Result<PathBuf> {
        let target = self.resolve(path)?;
        let root = self
            .root
            .canonicalize()
            .map_err(|e| BrainError::Hand(format!("workspace root: {e}")))?;
        let mut existing = target.clone();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| {
                    BrainError::Invalid(format!("file path {path} escapes the workspace"))
                })?
                .to_path_buf();
        }
        // For a symlink itself, validate its parent without following the final link. Reads
        // and writes reject that final link below; listings may describe it safely.
        let check = if existing == target
            && std::fs::symlink_metadata(&existing)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            existing.parent().unwrap_or(&existing).canonicalize()
        } else {
            existing.canonicalize()
        }
        .map_err(|e| BrainError::Hand(format!("resolve {path}: {e}")))?;
        if !check.starts_with(&root) {
            return Err(BrainError::Invalid(format!(
                "file path {path} resolves outside /workspace"
            )));
        }
        Ok(target)
    }

    fn public_path(&self, path: &Path) -> String {
        let rel = self.rel_of(path);
        if rel.is_empty() {
            "/workspace".into()
        } else {
            format!("/workspace/{rel}")
        }
    }

    fn file_entry(&self, path: &Path) -> Result<brain_protocol::session::FileEntry> {
        use brain_protocol::session::{FileEntry, FileEntryKind, Timestamp};
        let md = std::fs::symlink_metadata(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                BrainError::FileNotFound(self.public_path(path))
            } else {
                BrainError::Hand(format!("metadata {}: {e}", path.display()))
            }
        })?;
        let kind = if md.file_type().is_symlink() {
            FileEntryKind::Symlink
        } else if md.is_dir() {
            FileEntryKind::Dir
        } else {
            FileEntryKind::File
        };
        let (size, sha256) = if md.is_file() {
            use std::io::Read as _;
            let mut file = std::fs::File::open(path)
                .map_err(|e| BrainError::Hand(format!("read {}: {e}", path.display())))?;
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = file
                    .read(&mut buffer)
                    .map_err(|e| BrainError::Hand(format!("read {}: {e}", path.display())))?;
                if n == 0 {
                    break;
                }
                digest.update(&buffer[..n]);
            }
            let sha = hex::encode(digest.finalize())
                .parse()
                .map_err(|_| BrainError::Hand("sha256 conversion".into()))?;
            (Some(md.len()), Some(sha))
        } else {
            (None, None)
        };
        Ok(FileEntry {
            kind,
            modified_at: md
                .modified()
                .ok()
                .map(|at| Timestamp(chrono::DateTime::<chrono::Utc>::from(at))),
            path: self.public_path(path),
            sha256,
            size,
        })
    }

    fn list_workspace(&self, path: &str, recursive: bool) -> Result<WorkspaceListing> {
        let target = self.api_path(path)?;
        if !target.exists() {
            return Err(BrainError::FileNotFound(path.into()));
        }
        let mut entries = Vec::new();
        let max_depth = if recursive { usize::MAX } else { 1 };
        for item in walkdir::WalkDir::new(&target)
            .min_depth(0)
            .max_depth(max_depth)
            .follow_links(false)
        {
            let item = item.map_err(|e| BrainError::Hand(format!("list {path}: {e}")))?;
            entries.push(self.file_entry(item.path())?);
        }
        entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        Ok(WorkspaceListing {
            entries,
            source: brain_protocol::session::FileListSource::Hand,
            synced_ms: None,
        })
    }

    fn read_workspace(&self, path: &str, max_bytes: usize) -> Result<WorkspaceFile> {
        let target = self.api_path(path)?;
        let md = std::fs::symlink_metadata(&target)
            .map_err(|_| BrainError::FileNotFound(path.into()))?;
        if md.file_type().is_symlink() || !md.is_file() {
            return Err(BrainError::Invalid(format!(
                "file path {path} is not a regular file"
            )));
        }
        let canonical = target
            .canonicalize()
            .map_err(|e| BrainError::Hand(format!("resolve {path}: {e}")))?;
        if !canonical.starts_with(
            self.root
                .canonicalize()
                .map_err(|e| BrainError::Hand(format!("workspace root: {e}")))?,
        ) {
            return Err(BrainError::Invalid(format!(
                "file path {path} resolves outside /workspace"
            )));
        }
        if md.len() > max_bytes as u64 {
            return Err(BrainError::FileTooLarge { limit: max_bytes });
        }
        let bytes =
            std::fs::read(&target).map_err(|e| BrainError::Hand(format!("read {path}: {e}")))?;
        if bytes.len() > max_bytes {
            return Err(BrainError::FileTooLarge { limit: max_bytes });
        }
        Ok(WorkspaceFile {
            entry: self.file_entry(&target)?,
            bytes,
        })
    }

    fn write_workspace(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<brain_protocol::session::FileEntry> {
        let target = self.api_path(path)?;
        if target
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink() || m.is_dir())
            .unwrap_or(false)
        {
            return Err(BrainError::Invalid(format!(
                "file path {path} is not a regular file target"
            )));
        }
        let parent = target
            .parent()
            .ok_or_else(|| BrainError::Invalid("file path has no parent".into()))?;
        std::fs::create_dir_all(parent)
            .map_err(|e| BrainError::Hand(format!("create parent for {path}: {e}")))?;
        let tmp = parent.join(format!(".brain-upload-{}", crate::mint_id("tmp", 12)));
        std::fs::write(&tmp, bytes).map_err(|e| BrainError::Hand(format!("write {path}: {e}")))?;
        if let Err(first) = std::fs::rename(&tmp, &target) {
            // Windows does not replace an existing destination on every filesystem. Local
            // mode is explicitly non-durable/non-sandboxed; retain overwrite semantics.
            if target.exists() {
                std::fs::remove_file(&target)
                    .map_err(|e| BrainError::Hand(format!("replace {path}: {first}; {e}")))?;
                std::fs::rename(&tmp, &target)
                    .map_err(|e| BrainError::Hand(format!("replace {path}: {e}")))?;
            } else {
                return Err(BrainError::Hand(format!("write {path}: {first}")));
            }
        }
        self.file_entry(&target)
    }

    /// Executes one tool call. `emit` receives (stream_name, offset, chunk) for live output.
    pub async fn execute(
        self: &Arc<Self>,
        tool: &str,
        input: Value,
        cancel: &CancellationToken,
        emit: impl Fn(&str, u64, String) + Send + Sync + 'static,
    ) -> CallOutcome {
        let t0 = Instant::now();
        let done = |content: String, is_error: bool, exit: Option<i64>, t0: Instant| CallOutcome {
            outcome: (if is_error { "failed" } else { "completed" }).into(),
            value: None,
            content,
            is_error,
            exit_code: exit,
            duration_ms: t0.elapsed().as_millis() as u64,
            truncated: false,
            terminal: None,
        };
        match tool {
            "bash" => self.bash(input, cancel, emit, t0).await,
            _ => {
                // The file tools are synchronous filesystem work; off the async threads.
                let this = self.clone();
                let tool = tool.to_string();
                let joined = tokio::task::spawn_blocking(move || match tool.as_str() {
                    "read" => this.read(&input),
                    "write" => this.write(&input),
                    "edit" => this.edit(&input),
                    "glob" => this.glob(&input),
                    "grep" => this.grep(&input),
                    "ls" => this.ls(&input),
                    other => Err(BrainError::UndeclaredTool { name: other.into() }),
                })
                .await;
                match joined {
                    Ok(Ok((text, meta))) => {
                        let mut content = text;
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(&format!("[meta] {meta}"));
                        let mut outcome = done(content, false, None, t0);
                        outcome.value = Some(meta);
                        outcome
                    }
                    Ok(Err(e)) => done(e.to_string(), true, None, t0),
                    Err(e) => done(format!("tool task did not complete: {e}"), true, None, t0),
                }
            }
        }
    }

    async fn bash(
        &self,
        input: Value,
        cancel: &CancellationToken,
        emit: impl Fn(&str, u64, String) + Send + Sync + 'static,
        t0: Instant,
    ) -> CallOutcome {
        let fail = |msg: String| CallOutcome {
            outcome: "failed".into(),
            value: None,
            content: msg,
            is_error: true,
            exit_code: None,
            duration_ms: t0.elapsed().as_millis() as u64,
            truncated: false,
            terminal: None,
        };
        let Some(command) = input.get("command").and_then(|v| v.as_str()) else {
            return fail("bash: input.command is required".into());
        };
        let timeout_ms = input
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Its own process group, so cancel/timeout can kill the whole tree -- otherwise a
        // grandchild survives the kill AND holds the output pipes open.
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return fail(format!(
                    "could not spawn bash (local mode needs `bash` in PATH): {e}"
                ));
            }
        };

        let mut stdout_pipe = child.stdout.take().expect("piped");
        let mut stderr_pipe = child.stderr.take().expect("piped");
        let emit = Arc::new(emit);
        // Collectors write into shared buffers rather than returning them: a killed bash can
        // leave grandchildren holding the pipes (no EOF), and the call must not block on a
        // process it cannot reach.
        let out_buf = Arc::new(std::sync::Mutex::new(StreamBuf::default()));
        let err_buf = Arc::new(std::sync::Mutex::new(StreamBuf::default()));
        let out_task = {
            let emit = emit.clone();
            let buf = out_buf.clone();
            tokio::spawn(
                async move { collect_stream(&mut stdout_pipe, "stdout", &*emit, &buf).await },
            )
        };
        let err_task = {
            let emit = emit.clone();
            let buf = err_buf.clone();
            tokio::spawn(
                async move { collect_stream(&mut stderr_pipe, "stderr", &*emit, &buf).await },
            )
        };

        let mut cancelled = false;
        let mut timed_out = false;
        let status = tokio::select! {
            s = child.wait() => s.ok(),
            () = cancel.cancelled() => {
                cancelled = true;
                kill_tree(&mut child).await;
                child.wait().await.ok()
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                timed_out = true;
                kill_tree(&mut child).await;
                child.wait().await.ok()
            }
        };
        // Drain with a grace window, then abandon: an orphaned grandchild may hold the pipe
        // open indefinitely and must not hold the turn hostage.
        let drain = std::time::Duration::from_millis(1_500);
        if tokio::time::timeout(drain, out_task).await.is_err() {
            tracing::debug!("stdout collector abandoned (pipe held by an orphaned child)");
        }
        if tokio::time::timeout(drain, err_task).await.is_err() {
            tracing::debug!("stderr collector abandoned (pipe held by an orphaned child)");
        }
        let stdout = out_buf.lock().expect("stream buf").render();
        let stderr = err_buf.lock().expect("stream buf").render();

        let mut content = stdout;
        if !stderr.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str("[stderr]\n");
            content.push_str(&stderr);
        }
        if timed_out {
            content = format!("[command timed out after {timeout_ms} ms]\n{content}");
        }
        let exit_code = status.and_then(|s| s.code()).map(i64::from);
        let (outcome, is_error) = if cancelled {
            ("cancelled", true)
        } else if timed_out {
            ("completed", true)
        } else if exit_code == Some(0) {
            ("completed", false)
        } else {
            ("completed", true)
        };
        CallOutcome {
            outcome: outcome.into(),
            value: (!cancelled).then(|| serde_json::json!({"timed_out": timed_out})),
            content,
            is_error,
            exit_code,
            duration_ms: t0.elapsed().as_millis() as u64,
            truncated: false,
            terminal: None,
        }
    }

    // ---- file tools (each returns (text, meta_json)) ------------------------------------

    fn read(&self, input: &Value) -> Result<(String, Value)> {
        let path = str_arg(input, "path")?;
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let content = std::fs::read_to_string(self.resolve(path)?)
            .map_err(|e| BrainError::Invalid(format!("read {path}: {e}")))?;
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let start = offset.min(total.max(1));
        let end = (start + limit - 1).min(total);
        let text = if total == 0 || start > total {
            String::new()
        } else {
            lines[start - 1..end].join("\n")
        };
        Ok((
            text,
            json!({
                "total_lines": total,
                "start_line": start,
                "end_line": end,
                "truncated": end < total,
            }),
        ))
    }

    fn write(&self, input: &Value) -> Result<(String, Value)> {
        let path = str_arg(input, "path")?;
        let content = str_arg(input, "content")?;
        let target = self.resolve(path)?;
        let created = !target.exists();
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BrainError::Invalid(format!("write {path}: {e}")))?;
        }
        std::fs::write(&target, content)
            .map_err(|e| BrainError::Invalid(format!("write {path}: {e}")))?;
        Ok((
            format!("wrote {} bytes to {path}", content.len()),
            json!({ "bytes_written": content.len(), "created": created }),
        ))
    }

    fn edit(&self, input: &Value) -> Result<(String, Value)> {
        let path = str_arg(input, "path")?;
        let old = str_arg(input, "old_string")?;
        let new = str_arg(input, "new_string")?;
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let target = self.resolve(path)?;
        let content = std::fs::read_to_string(&target)
            .map_err(|e| BrainError::Invalid(format!("edit {path}: {e}")))?;
        let count = content.matches(old).count();
        if count == 0 {
            return Err(BrainError::Invalid(format!(
                "edit {path}: old_string not found"
            )));
        }
        if count > 1 && !replace_all {
            return Err(BrainError::Invalid(format!(
                "edit {path}: old_string occurs {count} times; pass replace_all or disambiguate"
            )));
        }
        let (updated, replacements) = if replace_all {
            (content.replace(old, new), count)
        } else {
            (content.replacen(old, new, 1), 1)
        };
        std::fs::write(&target, updated)
            .map_err(|e| BrainError::Invalid(format!("edit {path}: {e}")))?;
        Ok((
            format!("replaced {replacements} occurrence(s) in {path}"),
            json!({ "replacements": replacements }),
        ))
    }

    fn glob(&self, input: &Value) -> Result<(String, Value)> {
        let pattern = str_arg(input, "pattern")?;
        let base = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => self.resolve(p)?,
            None => self.root.clone(),
        };
        let max = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as usize;
        let glob = globset::GlobBuilder::new(pattern)
            .literal_separator(false)
            .build()
            .map_err(|e| BrainError::Invalid(format!("glob pattern: {e}")))?
            .compile_matcher();
        let mut matches = Vec::new();
        let mut truncated = false;
        for entry in walkdir::WalkDir::new(&base)
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&base)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if glob.is_match(&rel) {
                if matches.len() >= max {
                    truncated = true;
                    break;
                }
                matches.push(self.rel_of(entry.path()));
            }
        }
        Ok((
            matches.join("\n"),
            json!({ "matches": matches, "truncated": truncated }),
        ))
    }

    fn grep(&self, input: &Value) -> Result<(String, Value)> {
        let pattern = str_arg(input, "pattern")?;
        let ci = input
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mode = input
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("files");
        let context = input.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let max = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as usize;
        let base = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => self.resolve(p)?,
            None => self.root.clone(),
        };
        let file_filter = match input.get("glob").and_then(|v| v.as_str()) {
            Some(g) => Some(
                globset::GlobBuilder::new(g)
                    .literal_separator(false)
                    .build()
                    .map_err(|e| BrainError::Invalid(format!("grep glob: {e}")))?
                    .compile_matcher(),
            ),
            None => None,
        };
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(ci)
            .build()
            .map_err(|e| BrainError::Invalid(format!("grep pattern: {e}")))?;

        let mut out: Vec<String> = Vec::new();
        let mut results = 0usize;
        let mut truncated = false;
        'files: for entry in walkdir::WalkDir::new(&base)
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = self.rel_of(entry.path());
            if let Some(f) = &file_filter {
                let base_rel = entry
                    .path()
                    .strip_prefix(&base)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                if !f.is_match(&base_rel) {
                    continue;
                }
            }
            // Oversized or non-text files are skipped, like any sane grep default.
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() > 2 * 1024 * 1024 {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let content = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = content.lines().collect();
            let mut file_hits = 0usize;
            for (i, line) in lines.iter().enumerate() {
                if !re.is_match(line) {
                    continue;
                }
                file_hits += 1;
                if mode == "content" {
                    if results >= max {
                        truncated = true;
                        break 'files;
                    }
                    let lo = i.saturating_sub(context);
                    let hi = (i + context).min(lines.len() - 1);
                    for (j, l) in lines.iter().enumerate().take(hi + 1).skip(lo) {
                        out.push(format!("{rel}:{}: {l}", j + 1));
                    }
                    results += 1;
                }
            }
            if file_hits > 0 && mode != "content" {
                if results >= max {
                    truncated = true;
                    break;
                }
                results += 1;
                match mode {
                    "count" => out.push(format!("{rel}: {file_hits}")),
                    _ => out.push(rel),
                }
            }
        }
        Ok((
            out.join("\n"),
            json!({ "matches": out, "truncated": truncated }),
        ))
    }

    fn ls(&self, input: &Value) -> Result<(String, Value)> {
        let base = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => self.resolve(p)?,
            None => self.root.clone(),
        };
        let depth = input.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let hidden = input
            .get("hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max = input
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000) as usize;
        let mut entries = Vec::new();
        let mut truncated = false;
        for entry in walkdir::WalkDir::new(&base)
            .min_depth(1)
            .max_depth(depth.max(1))
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy();
            if !hidden && name.starts_with('.') {
                continue;
            }
            if entries.len() >= max {
                truncated = true;
                break;
            }
            let rel = self.rel_of(entry.path());
            entries.push(if entry.file_type().is_dir() {
                format!("{rel}/")
            } else {
                rel
            });
        }
        Ok((
            entries.join("\n"),
            json!({ "entries": entries, "truncated": truncated }),
        ))
    }
}

fn str_arg<'a>(input: &'a Value, key: &str) -> Result<&'a str> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| BrainError::Invalid(format!("input.{key} is required")))
}

/// Kills a bash invocation AND its descendants. A bare `kill` orphans grandchildren, which
/// keep running and keep the output pipes open.
async fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The child was spawned as its own process group leader; negative pid = the group.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

/// A bounded, tail-retained stream capture (the end of the output is where compilers and
/// tests put the verdict).
#[derive(Default)]
struct StreamBuf {
    tail: String,
    elided: usize,
}

impl StreamBuf {
    fn push(&mut self, text: &str) {
        self.tail.push_str(text);
        if self.tail.len() > MAX_STREAM_BYTES {
            let cut = self.tail.len() - MAX_STREAM_BYTES;
            let mut start = cut;
            while !self.tail.is_char_boundary(start) {
                start += 1;
            }
            self.elided += start;
            self.tail = self.tail.split_off(start);
        }
    }
    fn render(&self) -> String {
        if self.elided > 0 {
            format!(
                "[first {} bytes elided]
{}",
                self.elided, self.tail
            )
        } else {
            self.tail.clone()
        }
    }
}

/// Reads a pipe until EOF (or until abandoned), emitting chunks live into `emit` and the
/// shared buffer.
async fn collect_stream(
    pipe: &mut (impl tokio::io::AsyncRead + Unpin),
    name: &str,
    emit: &(impl Fn(&str, u64, String) + Send + Sync),
    into: &std::sync::Mutex<StreamBuf>,
) {
    let mut buf = [0u8; 8192];
    let mut offset = 0u64;
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                emit(name, offset, text.clone());
                offset += n as u64;
                into.lock().expect("stream buf").push(&text);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The adapter implementation
// ---------------------------------------------------------------------------------------------

/// Opens [`LocalHand`]s under one data directory. This is the zero-config default factory.
pub struct LocalFactory {
    pub data_dir: PathBuf,
}

impl LocalFactory {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }
}

#[async_trait::async_trait]
impl HandFactory for LocalFactory {
    async fn create(
        &self,
        spec: &HandSpec,
        seeds: &[SeedFile<'_>],
        bundles: &[ToolBundleFile<'_>],
    ) -> Result<serde_json::Value> {
        let h = LocalHand::open(&self.data_dir, &spec.session_id)?;
        for s in seeds {
            h.seed(s.path, s.bytes, s.mode)?;
        }
        for bundle in bundles {
            std::fs::write(
                h.bundles.join(format!("{}.mjs", bundle.checksum)),
                bundle.bytes,
            )
            .map_err(|error| BrainError::Hand(format!("stage tool bundle: {error}")))?;
        }
        Ok(serde_json::json!({ "v": 1 }))
    }

    async fn open(
        &self,
        spec: &HandSpec,
        _state: serde_json::Value,
    ) -> Result<Arc<dyn HandAdapter>> {
        Ok(LocalHand::open(&self.data_dir, &spec.session_id)?)
    }

    async fn purge(&self, session_id: &str) -> Result<()> {
        LocalHand::purge(&self.data_dir, session_id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl HandAdapter for LocalHand {
    async fn ensure_ready(&self) -> Result<Option<LostReport>> {
        // The directories exist from `open`; a local workspace cannot be "lost".
        Ok(None)
    }

    async fn call(
        &self,
        req: CallRequest,
        cancel: CancellationToken,
        sink: OutputSink,
    ) -> CallOutcome {
        // `execute` wants an Arc for its blocking tasks; re-wrap cheaply (paths only).
        let this = Arc::new(LocalHand {
            session_id: self.session_id.clone(),
            root: self.root.clone(),
            artifacts: self.artifacts.clone(),
            bundles: self.bundles.clone(),
        });
        this.execute(
            &req.tool,
            req.input,
            &cancel,
            move |stream, offset, text| sink(stream, offset, text),
        )
        .await
    }

    async fn release(&self) -> Result<()> {
        Ok(())
    }

    async fn list_files(&self, path: &str, recursive: bool) -> Result<WorkspaceListing> {
        self.list_workspace(path, recursive)
    }

    async fn read_file(&self, path: &str, max_bytes: usize) -> Result<WorkspaceFile> {
        self.read_workspace(path, max_bytes)
    }

    async fn write_file(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<brain_protocol::session::FileEntry> {
        self.write_workspace(path, bytes)
    }

    async fn persist(
        &self,
        name: &str,
        path: &str,
        media_type: Option<&str>,
    ) -> Result<ArtifactMeta> {
        let (bytes, sha256, target) = self.persist_file(name, path)?;
        Ok(ArtifactMeta {
            bytes,
            sha256,
            media_type: media_type.unwrap_or("application/octet-stream").to_string(),
            location: target.to_string_lossy().to_string(),
        })
    }

    fn hand_info(&self) -> brain_protocol::session::HandInfo {
        use brain_protocol::session::{HandInfo, HandShape, HandState};
        HandInfo {
            generation: Some(1),
            last_sync_at: None,
            live_jobs: Some(0),
            shape: HandShape::X1gb,
            started_at: None,
            state: HandState::Ready,
            wall_deadline_at: None,
        }
    }

    fn state(&self) -> serde_json::Value {
        // The workspace directory is self-describing; nothing to remember.
        serde_json::json!({ "v": 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash_available() -> bool {
        std::process::Command::new("bash")
            .arg("--version")
            .output()
            .is_ok()
    }

    fn hand() -> (tempdir::TempDirGuard, Arc<LocalHand>) {
        let dir = tempdir::TempDirGuard::new();
        let h = LocalHand::open(&dir.0, "ses_local_test").unwrap();
        (dir, h)
    }

    /// Minimal RAII temp dir (avoids a dev-dependency).
    mod tempdir {
        use std::path::PathBuf;
        pub struct TempDirGuard(pub PathBuf);
        impl TempDirGuard {
            pub fn new() -> Self {
                let p = std::env::temp_dir()
                    .join(format!("brain-local-test-{}", crate::mint_id("t", 10)));
                std::fs::create_dir_all(&p).unwrap();
                TempDirGuard(p)
            }
        }
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn paths_map_into_the_workspace_and_never_escape() {
        let (_g, h) = hand();
        assert!(h.resolve("a/b.txt").is_ok());
        assert!(h.resolve("/workspace/a.txt").is_ok());
        assert!(h.resolve("/home/agent/.bashrc").is_ok());
        assert!(h.resolve("/etc/passwd").is_err());
        assert!(h.resolve("../outside").is_err());
        assert!(h.resolve("a/../../outside").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn public_file_write_rejects_an_escaping_parent_symlink() {
        use std::os::unix::fs::symlink;

        let (guard, h) = hand();
        let outside = guard.0.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, h.root.join("escape")).unwrap();
        let result = h.write_workspace("/workspace/escape/nope.txt", b"nope");
        assert!(result.is_err());
        assert!(!outside.join("nope.txt").exists());
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[tokio::test]
    async fn write_read_edit_round_trip_with_meta() {
        let (_g, h) = hand();
        let cancel = CancellationToken::new();
        let out = h
            .execute(
                "write",
                json!({"path": "x.txt", "content": "one\ntwo\nthree"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("bytes_written"));

        let out = h
            .execute(
                "read",
                json!({"path": "x.txt", "offset": 2, "limit": 1}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.starts_with("two"), "{}", out.content);
        assert!(out.content.contains("\"total_lines\":3"));

        let out = h
            .execute(
                "edit",
                json!({"path": "x.txt", "old_string": "two", "new_string": "TWO"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = h
            .execute("read", json!({"path": "x.txt"}), &cancel, |_, _, _| {})
            .await;
        assert!(out.content.contains("TWO"));

        // A non-unique edit without replace_all is a typed failure the model can react to.
        h.execute(
            "write",
            json!({"path": "y.txt", "content": "a a"}),
            &cancel,
            |_, _, _| {},
        )
        .await;
        let out = h
            .execute(
                "edit",
                json!({"path": "y.txt", "old_string": "a", "new_string": "b"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("2 times"));
    }

    #[tokio::test]
    async fn glob_grep_ls_respect_bounds() {
        let (_g, h) = hand();
        let cancel = CancellationToken::new();
        for i in 0..5 {
            h.execute("write", json!({"path": format!("src/f{i}.rs"), "content": format!("fn f{i}() {{}} // needle")}), &cancel, |_, _, _| {})
                .await;
        }
        let out = h
            .execute(
                "glob",
                json!({"pattern": "src/*.rs"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.contains("src/f0.rs") && out.content.contains("src/f4.rs"));
        let out = h
            .execute(
                "glob",
                json!({"pattern": "src/*.rs", "max_results": 2}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.contains("\"truncated\":true"));

        let out = h
            .execute(
                "grep",
                json!({"pattern": "needle", "mode": "count"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.contains("src/f0.rs: 1"), "{}", out.content);
        let out = h
            .execute(
                "grep",
                json!({"pattern": "needle", "mode": "content", "glob": "src/f1.rs"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.contains("src/f1.rs:1:"), "{}", out.content);

        let out = h
            .execute("ls", json!({"depth": 2}), &cancel, |_, _, _| {})
            .await;
        assert!(out.content.contains("src/") && out.content.contains("src/f0.rs"));
    }

    #[tokio::test]
    async fn bash_runs_streams_and_reports_exit() {
        if !bash_available() {
            return;
        }
        let (_g, h) = hand();
        let cancel = CancellationToken::new();
        let chunks = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = chunks.clone();
        let out = h
            .execute(
                "bash",
                json!({"command": "echo local-ok && echo err-line >&2 && exit 3"}),
                &cancel,
                move |stream, _off, text| sink.lock().unwrap().push((stream.to_string(), text)),
            )
            .await;
        assert!(out.content.contains("local-ok"));
        assert!(out.content.contains("[stderr]"));
        assert_eq!(out.exit_code, Some(3));
        assert!(out.is_error, "non-zero exit is an error result");
        assert!(
            chunks
                .lock()
                .unwrap()
                .iter()
                .any(|(s, t)| s == "stdout" && t.contains("local-ok"))
        );
    }

    #[tokio::test]
    async fn bash_cancel_kills_the_process() {
        if !bash_available() {
            return;
        }
        let (_g, h) = hand();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            c2.cancel();
        });
        let t0 = Instant::now();
        let out = h
            .execute(
                "bash",
                json!({"command": "sleep 30"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert_eq!(out.outcome, "cancelled");
        assert!(t0.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn seed_and_persist_round_trip() {
        let (_g, h) = hand();
        h.seed("data/in.txt", b"seeded", None).unwrap();
        let cancel = CancellationToken::new();
        let out = h
            .execute(
                "read",
                json!({"path": "data/in.txt"}),
                &cancel,
                |_, _, _| {},
            )
            .await;
        assert!(out.content.contains("seeded"));
        let (bytes, sha, target) = h.persist_file("in.txt", "data/in.txt").unwrap();
        assert_eq!(bytes, 6);
        assert_eq!(sha.len(), 64);
        assert!(target.exists());
        assert!(h.persist_file("../evil", "data/in.txt").is_err());
    }
}
