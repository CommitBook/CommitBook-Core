# CommitBook PR Review: Comprehensive Issue Plan

## Context

This plan addresses 32 code review comments left on the CommitBook PR (`C/V1` -> `main`) by @copilot-pull-request-reviewer and @coderabbitai. CommitBook is a Rust workspace with 4 crates (`commitbook-core`, `commitbook-cli`, `commitbook-tui`, `commitbook-web`) that provides automated git commits for markdown notebooks.

The issues range from critical bugs (broken features, panics) to minor documentation mismatches. This plan groups them by priority and provides exact file locations and code changes needed.

---

## Priority 1: Critical Bugs (will crash or prevent features from working)

### Issue A: `now_iso()` produces incorrect timestamps (Comments #3, #25)
- **Rating: 10/10**
- **File:** `crates/commitbook-core/src/utils/datetime.rs:17`
- **Problem:** `Local::now().format("%Y-%m-%dT%H:%M:%SZ")` stamps local time with a `Z` (UTC) suffix. Every stored `created_at` and `last_commit` timestamp is semantically wrong in non-UTC timezones. Downstream relative-time calculations will be incorrect.
- **Plan:**
  1. Change `use chrono::Local;` to `use chrono::{Local, Utc};` at line 1
  2. Replace line 17: `Local::now().format(...)` -> `Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()`
  3. Update test `test_now_iso_pattern` in `datetime_tests.rs` — it checks `ends_with('Z')` which still passes, but verify it still works
- **Files to modify:** `crates/commitbook-core/src/utils/datetime.rs`

### Issue B: DateTime parsing silently fails in status display (Comment #16)
- **Rating: 10/10**
- **File:** `crates/commitbook-cli/src/commands/status.rs:51, 69`
- **Problem:** `DateTime::parse_from_str(last, "%Y-%m-%dT%H:%M:%SZ")` treats `Z` as a literal character, not a timezone. `parse_from_str` requires a timezone specifier like `%z` or `%+`. The `if let Ok(...)` silently swallows the parse failure, so "Last commit" and "Next commit" info never displays.
- **Plan:**
  1. Replace both occurrences of `DateTime::parse_from_str(last, "%Y-%m-%dT%H:%M:%SZ")` with `DateTime::parse_from_rfc3339(last)` (lines 51 and 69)
  2. `parse_from_rfc3339` correctly handles the `Z` suffix
- **Files to modify:** `crates/commitbook-cli/src/commands/status.rs`

### Issue C: UTF-8 truncate will panic on multi-byte characters (Comment #19)
- **Rating: 9/10**
- **File:** `crates/commitbook-core/src/ai/mod.rs:90-96`
- **Problem:** `&s[..max_len]` indexes by bytes. If `max_len` lands inside a multi-byte UTF-8 sequence (emoji, non-ASCII), Rust panics. AI providers may return emoji or unicode in commit messages.
- **Plan:**
  1. Replace the `truncate` function body:
     ```rust
     pub(crate) fn truncate(s: &str, max_len: usize) -> String {
         if s.len() <= max_len {
             s.to_string()
         } else {
             let mut end = max_len;
             while end > 0 && !s.is_char_boundary(end) {
                 end -= 1;
             }
             format!("{}...", &s[..end])
         }
     }
     ```
  2. Also fix `clean_message` at line 112-113 which does `msg.truncate(72)` — `String::truncate` also panics on non-char-boundary. Replace with the same boundary-safe approach.
- **Files to modify:** `crates/commitbook-core/src/ai/mod.rs`

### Issue D: Config page can never save — form/handler encoding mismatch (Comments #2, #31)
- **Rating: 9/10**
- **Files:** `crates/commitbook-web/src/routes.rs:238` + `crates/commitbook-web/templates/config.html:9`
- **Problem:** The HTML form at config.html:9 uses `hx-post="/api/config"` which sends `application/x-www-form-urlencoded` by default. But the handler at routes.rs:238 uses `Json(update): Json<ConfigUpdate>` which expects `application/json`. Axum will reject every request with a 422/415 error. The entire config page is non-functional.
- **Plan (Option: change handler to accept Form):**
  1. In `routes.rs:238`, change `Json(update): Json<ConfigUpdate>` to `axum::extract::Form(update): axum::extract::Form<ConfigUpdate>`
  2. Add `use axum::extract::Form;` to imports
  3. Ensure `ConfigUpdate` derives `Deserialize` (already does via serde) — Form deserialization works with serde
  4. Note: the `auto_push` field is sent as string `"true"`/`"false"` from the `<select>`. If `ConfigUpdate.auto_push` is `Option<bool>`, we need a custom deserializer or change it to `Option<String>` and parse manually. Check `models.rs` for the struct definition.
- **Files to modify:** `crates/commitbook-web/src/routes.rs`, possibly `crates/commitbook-web/src/models.rs`

---

## Priority 2: Security Issues

### Issue E: XSS in error message rendering (Comment #28)
- **Rating: 8/10**
- **File:** `crates/commitbook-web/src/routes.rs:251-253`
- **Problem:** `format!(r#"<div class="flash flash-error">Error: {}</div>"#, e)` interpolates an `anyhow::Error` directly into HTML. A malicious cron expression or crafted config value could inject `<script>` tags.
- **Plan:**
  1. HTML-escape the error string before interpolation. Use `askama::MarkupDisplay` or a simple manual escape:
     ```rust
     Err(e) => {
         let msg = e.to_string().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
         Html(format!(r#"<div class="flash flash-error">Error: {}</div>"#, msg))
     }
     ```
  2. Alternatively, add `html-escape` crate and use `html_escape::encode_safe(&e.to_string())`
- **Files to modify:** `crates/commitbook-web/src/routes.rs`

### Issue F: HTMX loaded from CDN without SRI (Comments #6, #30)
- **Rating: 3/10**
- **File:** `crates/commitbook-web/templates/base.html:7`
- **Problem:** `<script src="https://unpkg.com/htmx.org@2.0.4"></script>` has no integrity hash. Supply-chain risk if CDN is compromised.
- **Plan:**
  1. Upgrade to htmx 2.0.8 with official SRI from jsDelivr:
     ```html
     <script src="https://cdn.jsdelivr.net/npm/htmx.org@2.0.8/dist/htmx.min.js"
       integrity="sha384-/TgkGk7p307TH7EXJDuUlgG3Ce1UVolAOFopFekQkkXihi5u/6OCvVKyz1W+idaz"
       crossorigin="anonymous"></script>
     ```
- **Files to modify:** `crates/commitbook-web/templates/base.html`

---

## Priority 3: High-Impact Bugs

### Issue G: Blocking async runtime in Claude provider (Comment #18)
- **Rating: 8/10**
- **File:** `crates/commitbook-core/src/ai/claude.rs:69-93`
- **Problem:** `wait_with_timeout` uses `std::thread::sleep(Duration::from_millis(100))` in a polling loop inside an `async fn`. This blocks the tokio executor thread for up to 30 seconds. Additionally, `child.kill()` on line 85 is not followed by `child.wait()`, leaving a zombie process.
- **Plan:**
  1. Wrap the blocking operation in `tokio::task::spawn_blocking`:
     ```rust
     async fn generate(&self, summary: &ChangesSummary, repo_path: &Path) -> Result<String> {
         // ... build prompt and spawn child ...
         let output = tokio::task::spawn_blocking(move || {
             wait_with_timeout(child, CLAUDE_TIMEOUT)
         }).await??;
         // ... process output ...
     }
     ```
  2. Fix zombie: after `child.kill()`, add `let _ = child.wait();` before bailing
  3. Since `spawn_blocking` needs owned values, clone `repo_path` to `PathBuf` before the closure
- **Files to modify:** `crates/commitbook-core/src/ai/claude.rs`

### Issue H: Unquoted paths in crontab entries (Comments #8, #23)
- **Rating: 8/10**
- **File:** `crates/commitbook-core/src/cron/linux.rs:17`
- **Problem:** `format!("{} {} auto-commit --repo {}", schedule, bin_str, repo_str)` — paths with spaces will split into wrong arguments. E.g., `/home/user/My Notes` becomes two args.
- **Plan:**
  1. Quote both paths in the crontab entry:
     ```rust
     let entry = format!("{} \"{}\" auto-commit --repo \"{}\"", schedule, bin_str, repo_str);
     ```
  2. Update `filter_crontab_lines` — the marker check on line 34 (`line.trim() == marker`) should still work since comments aren't quoted. But line 38's `line.contains(&*repo_str)` will still match inside quotes. Verify the filter still works correctly.
  3. Update test assertions in `linux_tests.rs` to expect quoted paths
- **Files to modify:** `crates/commitbook-core/src/cron/linux.rs`, `crates/commitbook-core/src/cron/linux_tests.rs`

### Issue I: No XML escaping in macOS plist (Comment #13)
- **Rating: 8/10**
- **File:** `crates/commitbook-core/src/cron/macos.rs:47-83`
- **Problem:** `generate_plist` interpolates `bin_str`, `repo_str`, `path_env` directly into XML via `format!`. If any path contains `&`, `<`, or `>`, the plist is invalid and `launchctl load` fails silently.
- **Plan:**
  1. Add an XML escape helper function:
     ```rust
     fn xml_escape(s: &str) -> String {
         s.replace('&', "&amp;")
          .replace('<', "&lt;")
          .replace('>', "&gt;")
          .replace('"', "&quot;")
          .replace('\'', "&apos;")
     }
     ```
  2. Apply it to all interpolated values in `generate_plist`: `bin_str`, `repo_str`, `stdout`, `stderr`, `path_env`
  3. Note: the `label` is a SHA256 hash so it's safe, and `interval` is a number
- **Files to modify:** `crates/commitbook-core/src/cron/macos.rs`

### Issue J: `unsafe` blocks around `set_var` fail clippy (Comment #4)
- **Rating: 7/10**
- **File:** `crates/commitbook-cli/src/main.rs:99, 101`
- **Problem:** `unsafe { std::env::set_var("RUST_LOG", "debug") }` — `set_var` is safe in Rust 2021 edition. The `unsafe` blocks will trigger `unused_unsafe` lint, which fails CI (`clippy -D warnings`).
- **Plan:**
  1. Remove the `unsafe` blocks:
     ```rust
     if cli.verbose {
         std::env::set_var("RUST_LOG", "debug");
     } else if !cli.quiet {
         std::env::set_var("RUST_LOG", "info");
     }
     ```
  2. Note: In Rust 2024 edition, `set_var` becomes unsafe again. Check the edition in `Cargo.toml` — if it's 2021, remove `unsafe`. If it's 2024, keep it.
- **Files to modify:** `crates/commitbook-cli/src/main.rs`

### Issue K: Re-running setup doesn't update active scheduler (Comment #14)
- **Rating: 7/10**
- **File:** `crates/commitbook-cli/src/commands/setup.rs:33-45, 82-97`
- **Problem:** If a user overwrites an existing setup while a cron/launchd job is running, `finish_setup` only rewrites config files. The old scheduler keeps running with the old schedule. Running `start` later exits early because `cron::is_loaded()` returns true.
- **Plan:**
  1. In the overwrite branch (after line 44), check if scheduler is active and reinstall:
     ```rust
     // After the user confirms overwrite, before calling finish_setup:
     let was_running = cron::is_loaded(repo_path);
     // ... call finish_setup ...
     if was_running {
         // Reinstall scheduler with new schedule
         let commitbook_bin = std::env::current_exe()?;
         cron::install(repo_path, schedule, &commitbook_bin)?;
         println!("  {} Scheduler updated with new schedule", "OK".green().bold());
     }
     ```
  2. This requires `finish_setup` to return the schedule, or we restructure to check after `finish_setup` returns
- **Files to modify:** `crates/commitbook-cli/src/commands/setup.rs`

### Issue L: HTMX custom event trigger doesn't work (Comment #33)
- **Rating: 7/10**
- **File:** `crates/commitbook-web/templates/dashboard.html:6, 17, 22`
- **Problem:** Start/Stop buttons trigger `htmx.trigger('#status-section','htmx:trigger')` but the status div only listens for `"every 5s"`. The custom event is never handled, so the UI doesn't refresh after start/stop.
- **Plan:**
  1. Add custom event to status section trigger:
     ```html
     <div hx-get="/htmx/status" hx-trigger="every 5s, refresh-status from:body" hx-swap="innerHTML" id="status-section">
     ```
  2. Update buttons to trigger the custom event on `document.body`:
     ```html
     hx-on::after-request="htmx.trigger(document.body,'refresh-status')"
     ```
- **Files to modify:** `crates/commitbook-web/templates/dashboard.html`

---

## Priority 4: Medium-Impact Issues

### Issue M: Global config save error swallowed in uninstall (Comment #17)
- **Rating: 6/10**
- **File:** `crates/commitbook-cli/src/commands/uninstall.rs:52`
- **Problem:** `let _ = global.save();` ignores save failure. Uninstall reports success but global state may be stale.
- **Plan:**
  1. Change `let _ = global.save();` to `global.save()?;`
- **Files to modify:** `crates/commitbook-cli/src/commands/uninstall.rs`

### Issue N: `set_repo_enabled` silently no-ops for missing repos (Comment #15)
- **Rating: 6/10**
- **File:** `crates/commitbook-cli/src/commands/start.rs:46` + `crates/commitbook-core/src/config/global.rs:126-131`
- **Problem:** `GlobalConfig::set_repo_enabled` only updates existing entries via `get_mut`. If the repo was never registered (or global config was reset), it returns `Ok(())` silently. The start command proceeds but global state is stale.
- **Plan:**
  1. In `start.rs`, after line 46, add a fallback:
     ```rust
     let mut global = GlobalConfig::load()?;
     if global.repos.contains_key(&repo_str) {
         global.set_repo_enabled(&repo_str, true)?;
     } else {
         global.register_repo(&repo_str, &config.schedule)?;
     }
     ```
- **Files to modify:** `crates/commitbook-cli/src/commands/start.rs`

### Issue O: Pagination broken for filtered log queries (Comment #10)
- **Rating: 6/10**
- **File:** `crates/commitbook-web/src/routes.rs:225-234`
- **Problem:** `load_log_entries` applies `limit`/`offset` first, then `entries.retain(|e| e.level == *level)` filters the page. `total` is set to the post-filter count. This means: (a) filtered results are fewer than `limit`, (b) `total` doesn't reflect the full dataset, (c) pagination breaks.
- **Plan:**
  1. Move filtering before pagination. In `api_logs`:
     ```rust
     let mut entries = load_log_entries(&state.repo_path, usize::MAX, 0); // load all
     if let Some(ref level) = query.level {
         entries.retain(|e| e.level == *level);
     }
     let total = entries.len();
     let entries: Vec<_> = entries.into_iter().skip(offset).take(limit).collect();
     ```
  2. This trades performance for correctness. For the MVP this is acceptable. The performance issue (Issue U) can optimize this later.
- **Files to modify:** `crates/commitbook-web/src/routes.rs`

### Issue P: Custom cron expressions overwritten on config save (Comment #32)
- **Rating: 6/10**
- **File:** `crates/commitbook-web/templates/config.html:12-20`
- **Problem:** The `<select>` only has 7 preset options. If the stored schedule is a custom cron expression (set via CLI), the browser submits a preset value on save, silently replacing it.
- **Plan:**
  1. Add a conditional custom option in the Askama template:
     ```html
     <select name="schedule" id="schedule">
         {% if schedule != "*/5 * * * *" && schedule != "*/15 * * * *" && schedule != "*/30 * * * *" && schedule != "0 * * * *" && schedule != "0 */2 * * *" && schedule != "0 */4 * * *" && schedule != "0 9 * * *" %}
         <option value="{{ schedule }}" selected>Custom ({{ schedule }})</option>
         {% endif %}
         <option value="*/5 * * * *" {% if schedule == "*/5 * * * *" %}selected{% endif %}>Every 5 minutes</option>
         <!-- ... rest of options ... -->
     </select>
     ```
  2. Note: Askama uses `&&` for logical AND in conditionals
- **Files to modify:** `crates/commitbook-web/templates/config.html`

### Issue Q: Fragile binary path derivation in TUI and web (Comments #26, #29)
- **Rating: 5/10**
- **Files:** `crates/commitbook-tui/src/app.rs:166-171` + `crates/commitbook-web/src/routes.rs:258-261`
- **Problem:** Both derive the `commitbook` binary from `current_exe().parent().join("commitbook")`. This breaks if binaries aren't co-located (e.g., different install paths, system package managers).
- **Plan:**
  1. In `app.rs:163-175`, use `which::which`:
     ```rust
     fn toggle_scheduler(&mut self) {
         if self.running {
             let _ = cron::uninstall(&self.repo_path, None);
         } else {
             let commitbook_bin = which::which("commitbook")
                 .or_else(|_| std::env::current_exe().map(|bin|
                     bin.parent().map(|p| p.join("commitbook")).unwrap_or(bin)
                 ));
             if let Ok(bin) = commitbook_bin {
                 let _ = cron::install(&self.repo_path, &self.schedule, &bin);
             }
         }
         self.refresh();
     }
     ```
  2. Apply same pattern in `routes.rs:258-261`
  3. Add `which` to `commitbook-tui/Cargo.toml` and `commitbook-web/Cargo.toml` dependencies (it's already a transitive dep via `commitbook-core`)
- **Files to modify:** `crates/commitbook-tui/src/app.rs`, `crates/commitbook-web/src/routes.rs`, `crates/commitbook-tui/Cargo.toml`, `crates/commitbook-web/Cargo.toml`

### Issue R: Wrong command name in error messages (Comments #5, #12)
- **Rating: 5/10**
- **Files:** `crates/commitbook-web/src/main.rs:34` + `crates/commitbook-tui/src/main.rs:24`
- **Problem:** Both say `"Run \`commitbook init\` first."` but the actual command is `commitbook setup` (as defined in `cli/main.rs:33`).
- **Plan:**
  1. `web/main.rs:34`: Change `"commitbook init"` to `"commitbook setup"`
  2. `tui/main.rs:24`: Change `"commitbook init"` to `"commitbook setup"`
- **Files to modify:** `crates/commitbook-web/src/main.rs`, `crates/commitbook-tui/src/main.rs`

### Issue S: `max_log_files` naming mismatch (Comment #11)
- **Rating: 5/10**
- **File:** `crates/commitbook-core/src/config/local.rs:35`
- **Problem:** Field is named `max_log_files` but it's used as `max_log_days` (passed to `FileLogger::new(repo_path, config.logging.max_log_files)` which uses it for date-based cleanup, not file-count). Confusing for users editing config.
- **Plan:**
  1. Rename `max_log_files` to `max_log_days` in `LoggingSettings` struct (line 35)
  2. Add `#[serde(alias = "max_log_files")]` for backwards compatibility with existing configs
  3. Update all references across the codebase (grep for `max_log_files`)
- **Files to modify:** `crates/commitbook-core/src/config/local.rs`, and any files referencing `logging.max_log_files`

### Issue T: Terminal not restored on setup failure in TUI (Comment #27)
- **Rating: 5/10**
- **File:** `crates/commitbook-tui/src/main.rs:29-54`
- **Problem:** If `ratatui::Terminal::new(backend)` at line 40 fails, the function returns via `?` before the restore block at line 44-52. The shell stays in raw/alternate-screen mode.
- **Plan:**
  1. Add a RAII guard struct:
     ```rust
     struct TerminalGuard;
     impl Drop for TerminalGuard {
         fn drop(&mut self) {
             let _ = crossterm::terminal::disable_raw_mode();
             let _ = crossterm::execute!(
                 std::io::stdout(),
                 crossterm::terminal::LeaveAlternateScreen,
                 crossterm::event::DisableMouseCapture
             );
         }
     }
     ```
  2. Create the guard right after `enable_raw_mode()` succeeds
  3. The existing restore code can remain as-is for the happy path (the guard handles failures)
- **Files to modify:** `crates/commitbook-tui/src/main.rs`

---

## Priority 5: Low-Impact Issues

### Issue U: `read_entries` loads all logs into memory (Comment #9)
- **Rating: 4/10**
- **File:** `crates/commitbook-core/src/logger/file_logger.rs:155-193`
- **Problem:** Reads every `.log` file and every line into `all_lines` before applying `offset`/`limit`. With many days of logs and web UI polling, this is O(total_logs) per request.
- **Plan:**
  1. Add early termination: track how many entries we've skipped and collected, stop reading files once `offset + limit` is reached:
     ```rust
     let mut collected = 0;
     let mut skipped = 0;
     let mut result = Vec::new();
     for file in &log_files {
         // ... read and reverse lines ...
         for line in lines {
             if skipped < offset { skipped += 1; continue; }
             result.push(line);
             collected += 1;
             if collected >= limit { return Ok(result); }
         }
     }
     ```
- **Files to modify:** `crates/commitbook-core/src/logger/file_logger.rs`

### Issue V: Test assertions have wrong ordering (Comment #24)
- **Rating: 4/10**
- **File:** `crates/commitbook-core/src/logger/file_logger_tests.rs:137-140`
- **Problem:** The test expects `entries[0].contains("new2")` (newest first within a file), which is actually correct — `read_entries` reverses lines within each file (line 182 of `file_logger.rs`). The CodeRabbit comment was wrong here — the implementation does reverse. **However**, verify this by checking: `read_entries` does `lines.reverse()` at line 182, so within today's file, `new2` (last written) becomes first. The test assertions are correct.
- **Plan:** No change needed — the test matches the implementation. The reviewer misread the code.

### Issue W: PR description says Actix but code uses Axum (Comment #7)
- **Rating: 2/10**
- **File:** PR description / `crates/commitbook-web/Cargo.toml:15`
- **Problem:** Documentation mismatch only.
- **Plan:** Update PR description to say "Axum" instead of "Actix". No code change needed.

### Issue X: Hardcoded `/tmp` in config test (Comment #20)
- **Rating: 2/10**
- **File:** `crates/commitbook-core/src/config/mod_tests.rs:5-6`
- **Problem:** Uses hardcoded `/tmp` which is platform-dependent, and assertion `contains("tmp")` is weak.
- **Plan:**
  1. Use `tempfile::tempdir()`:
     ```rust
     #[test]
     fn test_resolve_repo_path_with_value() {
         let tmp = tempfile::tempdir().unwrap();
         let result = resolve_repo_path(Some(tmp.path())).unwrap();
         assert_eq!(result, tmp.path().canonicalize().unwrap());
     }
     ```
- **Files to modify:** `crates/commitbook-core/src/config/mod_tests.rs`

### Issue Y: Missing regression tests for crontab path handling (Comments #21, #22)
- **Rating: 2/10**
- **File:** `crates/commitbook-core/src/cron/linux_tests.rs`
- **Problem:** No tests for paths with spaces or prefix-matching false positives.
- **Plan:**
  1. Add test for spaces (after Issue H quotes paths):
     ```rust
     #[test]
     fn test_build_entry_repo_path_with_spaces() {
         let (_, entry) = build_crontab_entry(
             Path::new("/home/user/My Notes"),
             "0 * * * *",
             Path::new("/usr/bin/commitbook"),
         );
         assert!(entry.contains("\"") || entry.contains("'"));
         assert!(entry.contains("My Notes"));
     }
     ```
  2. Add test for prefix false positive:
     ```rust
     #[test]
     fn test_filter_does_not_remove_prefix_matching_repo() {
         let crontab = "# CommitBook: /tmp/repository\n0 * * * * /usr/bin/commitbook auto-commit --repo /tmp/repository";
         let result = filter_crontab_lines(crontab, Path::new("/tmp/repo"));
         assert!(result.contains("/tmp/repository"));
     }
     ```
  3. Note: the prefix test will currently fail because `filter_crontab_lines` line 38 uses `line.contains(&*repo_str)` which matches `/tmp/repo` inside `/tmp/repository`. This is a real bug that should also be fixed by matching on the full `--repo <path>` argument.
- **Files to modify:** `crates/commitbook-core/src/cron/linux_tests.rs`, `crates/commitbook-core/src/cron/linux.rs` (fix the substring matching bug)

---

## Execution Order

The recommended order for implementation, grouped by file to minimize context switching:

### Batch 1: Core library fixes (most impactful, no dependencies)
1. **Issue A** — `datetime.rs` UTC fix
2. **Issue C** — `ai/mod.rs` UTF-8 safe truncate
3. **Issue G** — `ai/claude.rs` async/zombie fix
4. **Issue H + Y** — `cron/linux.rs` quote paths + fix substring match + tests
5. **Issue I** — `cron/macos.rs` XML escaping
6. **Issue S** — `config/local.rs` rename `max_log_files`
7. **Issue U** — `file_logger.rs` early termination

### Batch 2: CLI fixes
8. **Issue J** — `cli/main.rs` remove `unsafe`
9. **Issue B** — `status.rs` DateTime parsing
10. **Issue K** — `setup.rs` reinstall scheduler on overwrite
11. **Issue N** — `start.rs` handle missing global entry
12. **Issue M** — `uninstall.rs` propagate save error

### Batch 3: TUI fixes
13. **Issue R** (TUI) — `tui/main.rs` fix command name
14. **Issue T** — `tui/main.rs` terminal guard
15. **Issue Q** (TUI) — `tui/app.rs` use `which::which`

### Batch 4: Web fixes
16. **Issue D** — `routes.rs` + `config.html` form encoding
17. **Issue E** — `routes.rs` XSS escape
18. **Issue O** — `routes.rs` pagination fix
19. **Issue Q** (web) — `routes.rs` binary path
20. **Issue R** (web) — `web/main.rs` fix command name
21. **Issue F** — `base.html` HTMX SRI
22. **Issue L** — `dashboard.html` event trigger
23. **Issue P** — `config.html` custom schedule preservation

### Batch 5: Tests and docs
24. **Issue X** — `mod_tests.rs` tempdir
25. **Issue W** — PR description update

---

## Verification Plan

After all changes are applied:

1. **Run full test suite:** `cargo test --workspace`
2. **Run clippy:** `cargo clippy --workspace -- -D warnings` (verifies Issue J)
3. **Run fmt check:** `cargo fmt --all -- --check`
4. **Manual verification of web UI:**
   - Start `commitbook-web --repo <test-repo>`
   - Navigate to `/config`, change schedule, click Save (verifies Issue D)
   - Check that error messages are HTML-escaped (Issue E)
   - Click Start/Stop on dashboard, verify immediate status refresh (Issue L)
5. **Manual verification of CLI:**
   - Run `commitbook status` on a repo with a last_commit timestamp (verifies Issue B)
   - Run `commitbook setup` on an already-set-up repo with running scheduler (verifies Issue K)
6. **Verify timestamps:** `commitbook setup` a new repo, check `.CommitBook/config.toml` — `created_at` should be in actual UTC (Issue A)

---

## Files Modified (complete list)

| File | Issues |
|------|--------|
| `crates/commitbook-core/src/utils/datetime.rs` | A |
| `crates/commitbook-core/src/ai/mod.rs` | C |
| `crates/commitbook-core/src/ai/claude.rs` | G |
| `crates/commitbook-core/src/cron/linux.rs` | H, Y |
| `crates/commitbook-core/src/cron/linux_tests.rs` | H, Y |
| `crates/commitbook-core/src/cron/macos.rs` | I |
| `crates/commitbook-core/src/config/local.rs` | S |
| `crates/commitbook-core/src/logger/file_logger.rs` | U |
| `crates/commitbook-core/src/config/mod_tests.rs` | X |
| `crates/commitbook-cli/src/main.rs` | J |
| `crates/commitbook-cli/src/commands/status.rs` | B |
| `crates/commitbook-cli/src/commands/setup.rs` | K |
| `crates/commitbook-cli/src/commands/start.rs` | N |
| `crates/commitbook-cli/src/commands/uninstall.rs` | M |
| `crates/commitbook-tui/src/main.rs` | R, T |
| `crates/commitbook-tui/src/app.rs` | Q |
| `crates/commitbook-tui/Cargo.toml` | Q |
| `crates/commitbook-web/src/routes.rs` | D, E, O, Q |
| `crates/commitbook-web/src/main.rs` | R |
| `crates/commitbook-web/src/models.rs` | D (possibly) |
| `crates/commitbook-web/Cargo.toml` | Q |
| `crates/commitbook-web/templates/base.html` | F |
| `crates/commitbook-web/templates/config.html` | D, P |
| `crates/commitbook-web/templates/dashboard.html` | L |
