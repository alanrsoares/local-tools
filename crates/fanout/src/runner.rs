//! Concurrent execution engine and UI event loop for `fanout`.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use local_common::is_terminal;

use crate::cli::Options;
use crate::ui::{
    draw_meter, format_duration, phase_icon, phase_text, strip_ansi, terminal_columns,
    visible_width, CursorGuard, Outcome, Styles,
};
use crate::workspace::TaskSpec;

pub enum Event {
    TaskStarted {
        index: usize,
    },
    TaskLine {
        index: usize,
        text: String,
        is_stderr: bool,
    },
    TaskFinished {
        index: usize,
        outcome: Outcome,
        duration: Duration,
        exit_code: i32,
        tail_lines: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub phase: Outcome,
    pub began: Option<Instant>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub name: String,
    pub outcome: Outcome,
    pub duration: Duration,
    pub color_idx: usize,
}

pub fn execute_tasks(tasks: Vec<TaskSpec>, opts: Options) -> i32 {
    let num_tasks = tasks.len();
    if num_tasks == 0 {
        println!("no tasks to run");
        return 0;
    }

    let stdout_is_tty = is_terminal(&io::stdout());
    let color_enabled = if opts.color {
        true
    } else if opts.no_color || opts.compact {
        false
    } else {
        stdout_is_tty && std::env::var_os("NO_COLOR").is_none()
    };

    let styles = Styles::new(color_enabled);
    let interactive = stdout_is_tty && !opts.compact;
    let _cursor_guard = CursorGuard::new(interactive);

    let gutter = tasks.iter().map(|t| t.name.len()).max().unwrap_or(10);
    let start_time = Instant::now();

    // Print plan header if not in compact mode
    if !opts.compact {
        print_plan_header(&tasks, &opts, &styles);
    }

    // Shared cancellation and queue
    let bail_flag = Arc::new(AtomicBool::new(false));
    let (tx, rx): (Sender<Event>, Receiver<Event>) = mpsc::channel();

    // Prioritize tasks (longest poles first)
    let mut task_indices: Vec<usize> = (0..num_tasks).collect();
    task_indices.sort_by(|&a, &b| tasks[b].estimated_cost.cmp(&tasks[a].estimated_cost));

    let queue = Arc::new(Mutex::new(VecDeque::from(task_indices)));
    let active_workers = Arc::new(AtomicUsize::new(0));

    let pool_size = opts.jobs.min(num_tasks);
    let mut worker_handles = Vec::new();

    for _ in 0..pool_size {
        let q = Arc::clone(&queue);
        let tasks_ref = tasks.clone();
        let tx_clone = tx.clone();
        let bail_ref = Arc::clone(&bail_flag);
        let active = Arc::clone(&active_workers);
        let timeout_ms = opts.timeout_ms;
        let tail_cap = opts.tail_lines;
        let bail_on_fail = opts.bail;
        let color_opt = color_enabled;

        let handle = thread::spawn(move || loop {
            if bail_ref.load(Ordering::SeqCst) {
                break;
            }

            let next_idx = {
                let mut lock = match q.lock() {
                    Ok(l) => l,
                    Err(poisoned) => poisoned.into_inner(),
                };
                lock.pop_front()
            };

            let idx = match next_idx {
                Some(i) => i,
                None => break,
            };

            if bail_ref.load(Ordering::SeqCst) {
                let _ = tx_clone.send(Event::TaskFinished {
                    index: idx,
                    outcome: Outcome::Cancelled,
                    duration: Duration::ZERO,
                    exit_code: 0,
                    tail_lines: Vec::new(),
                });
                continue;
            }

            active.fetch_add(1, Ordering::SeqCst);
            let _ = tx_clone.send(Event::TaskStarted { index: idx });

            let spec = &tasks_ref[idx];
            let task_start = Instant::now();

            let (outcome, exit_code, tail_lines) = run_single_task(
                spec, timeout_ms, tail_cap, &tx_clone, idx, &bail_ref, color_opt,
            );

            let duration = task_start.elapsed();
            active.fetch_sub(1, Ordering::SeqCst);

            if outcome.is_bad() && bail_on_fail {
                bail_ref.store(true, Ordering::SeqCst);
            }

            let _ = tx_clone.send(Event::TaskFinished {
                index: idx,
                outcome,
                duration,
                exit_code,
                tail_lines,
            });
        });
        worker_handles.push(handle);
    }
    drop(tx); // Drop initial sender so rx can finish when workers drop theirs

    // TUI event loop state
    let mut states: Vec<TaskState> = vec![
        TaskState {
            phase: Outcome::Pending,
            began: None,
            duration: None,
        };
        num_tasks
    ];
    let mut buffered_logs: Vec<Vec<String>> = vec![Vec::new(); num_tasks];
    let mut results: Vec<Option<TaskResult>> = vec![None; num_tasks];
    let mut painted_lines = 0;
    let mut frame = 0;
    let mut last_tick = Instant::now();

    let mut out = io::BufWriter::with_capacity(64 * 1024, io::stdout());

    // Initial paint for interactive mode
    if interactive {
        painted_lines = paint_status_block(
            &mut out, &tasks, &states, frame, start_time, gutter, &styles,
        );
        let _ = out.flush();
    }

    let mut batch = Vec::new();
    let mut done = false;

    loop {
        batch.clear();
        let timeout = Duration::from_millis(80);
        match rx.recv_timeout(timeout) {
            Ok(evt) => {
                batch.push(evt);
                while let Ok(next_evt) = rx.try_recv() {
                    batch.push(next_evt);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                done = true;
            }
        }

        let mut log_output = String::new();
        let mut state_changed = false;

        for event in batch.drain(..) {
            match event {
                Event::TaskStarted { index } => {
                    states[index].phase = Outcome::Running;
                    states[index].began = Some(Instant::now());
                    state_changed = true;
                    if !interactive && !opts.compact {
                        let spec = &tasks[index];
                        log_output.push_str(&format!(
                            "{} {}{} {} started\n",
                            styles.cyan(),
                            styles.task_color(spec.color_idx),
                            spec.name,
                            styles.reset()
                        ));
                    }
                }
                Event::TaskLine {
                    index,
                    text,
                    is_stderr,
                } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    let spec = &tasks[index];
                    let prefix = format!(
                        "{}{}{}{}{} {} ",
                        styles.task_color(spec.color_idx),
                        styles.bold(),
                        spec.name.pad_right(gutter),
                        styles.reset(),
                        styles.gray(),
                        if is_stderr { "┃" } else { "│" }
                    );
                    let rendered = format!("{}{}{}\n", prefix, styles.reset(), text);
                    if interactive {
                        log_output.push_str(&rendered);
                    } else if !opts.compact {
                        buffered_logs[index].push(rendered);
                    }
                }
                Event::TaskFinished {
                    index,
                    outcome,
                    duration,
                    exit_code,
                    tail_lines,
                } => {
                    states[index].phase = outcome;
                    states[index].duration = Some(duration);
                    state_changed = true;
                    let spec = &tasks[index];

                    results[index] = Some(TaskResult {
                        name: spec.name.clone(),
                        outcome,
                        duration,
                        color_idx: spec.color_idx,
                    });

                    if interactive {
                        let verdict =
                            format_verdict(spec, outcome, duration, exit_code, gutter, &styles);
                        log_output.push_str(&verdict);
                    } else if opts.compact {
                        if outcome != Outcome::Passed && outcome != Outcome::Cancelled {
                            log_output.push_str(&format!(
                                "{} {} ({})\n",
                                match outcome {
                                    Outcome::Failed(_) => "FAILED",
                                    Outcome::Timeout => "TIMEOUT",
                                    _ => "ERROR",
                                },
                                spec.name,
                                format_duration(duration)
                            ));
                            if !tail_lines.is_empty() {
                                log_output.push_str(&format!("--- {}\n", spec.name));
                                for line in &tail_lines {
                                    log_output.push_str(line);
                                    log_output.push('\n');
                                }
                            }
                        }
                    } else {
                        // Non-interactive standard output
                        for line in &buffered_logs[index] {
                            log_output.push_str(line);
                        }
                        let verdict =
                            format_verdict(spec, outcome, duration, exit_code, gutter, &styles);
                        log_output.push_str(&verdict);
                    }
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(80) {
            frame = frame.wrapping_add(1);
            last_tick = Instant::now();
            state_changed = true;
        }

        if interactive {
            if !log_output.is_empty() || state_changed {
                clear_pinned(&mut out, painted_lines);
                if !log_output.is_empty() {
                    let _ = out.write_all(log_output.as_bytes());
                }
                painted_lines = paint_status_block(
                    &mut out, &tasks, &states, frame, start_time, gutter, &styles,
                );
                let _ = out.flush();
            }
        } else if !log_output.is_empty() {
            let _ = out.write_all(log_output.as_bytes());
            let _ = out.flush();
        }

        if done {
            break;
        }
    }

    // Wait for all workers to finish
    for handle in worker_handles {
        let _ = handle.join();
    }

    if interactive {
        clear_pinned(&mut out, painted_lines);
        let _ = out.flush();
    }

    let final_results: Vec<TaskResult> = results.into_iter().flatten().collect();
    let total_elapsed = start_time.elapsed();

    print_verdict_summary(&final_results, &opts, total_elapsed, gutter, &styles);

    let has_failures = final_results.iter().any(|r| r.outcome.is_bad());
    if has_failures {
        1
    } else {
        0
    }
}

fn run_single_task(
    spec: &TaskSpec,
    timeout_ms: u64,
    tail_cap: usize,
    tx: &Sender<Event>,
    idx: usize,
    bail_ref: &Arc<AtomicBool>,
    color_opt: bool,
) -> (Outcome, i32, Vec<String>) {
    let mut cmd = Command::new(&spec.runner_bin);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Each task owns a process group so cancellation also reaches package
    // managers and their subprocesses, not only the immediate runner.
    local_common::process::own_process_group(&mut cmd);

    if color_opt {
        cmd.env("FORCE_COLOR", "1");
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(Event::TaskLine {
                index: idx,
                text: format!("spawn failed: {e}"),
                is_stderr: true,
            });
            return (Outcome::Failed(1), 1, vec![format!("spawn error: {e}")]);
        }
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let collected_tail = Arc::new(Mutex::new(VecDeque::new()));

    let t1_tail = Arc::clone(&collected_tail);
    let t1_tx = tx.clone();
    let h_stdout = thread::spawn(move || {
        if let Some(pipe) = stdout_pipe {
            let reader = BufReader::new(pipe);
            for line in reader.lines().map_while(Result::ok) {
                {
                    let mut t = match t1_tail.lock() {
                        Ok(l) => l,
                        Err(p) => p.into_inner(),
                    };
                    t.push_back(line.clone());
                    if tail_cap > 0 && t.len() > tail_cap {
                        t.pop_front();
                    }
                }
                let _ = t1_tx.send(Event::TaskLine {
                    index: idx,
                    text: line,
                    is_stderr: false,
                });
            }
        }
    });

    let t2_tail = Arc::clone(&collected_tail);
    let t2_tx = tx.clone();
    let h_stderr = thread::spawn(move || {
        if let Some(pipe) = stderr_pipe {
            let reader = BufReader::new(pipe);
            for line in reader.lines().map_while(Result::ok) {
                {
                    let mut t = match t2_tail.lock() {
                        Ok(l) => l,
                        Err(p) => p.into_inner(),
                    };
                    t.push_back(line.clone());
                    if tail_cap > 0 && t.len() > tail_cap {
                        t.pop_front();
                    }
                }
                let _ = t2_tx.send(Event::TaskLine {
                    index: idx,
                    text: line,
                    is_stderr: true,
                });
            }
        }
    });

    let task_start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    let (outcome, exit_code) = loop {
        if bail_ref.load(Ordering::SeqCst) {
            local_common::process::terminate(&mut child);
            break (Outcome::Cancelled, 0);
        }

        if task_start.elapsed() >= timeout {
            local_common::process::terminate(&mut child);
            break (Outcome::Timeout, 124);
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    break (Outcome::Passed, 0);
                } else {
                    let code = status.code().unwrap_or(1);
                    break (Outcome::Failed(code), code);
                }
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                break (Outcome::Failed(1), 1);
            }
        }
    };

    let _ = h_stdout.join();
    let _ = h_stderr.join();

    let tail_lines: Vec<String> = {
        let lock = match collected_tail.lock() {
            Ok(l) => l,
            Err(p) => p.into_inner(),
        };
        lock.iter().cloned().collect()
    };

    (outcome, exit_code, tail_lines)
}

fn clear_pinned(out: &mut impl Write, count: usize) {
    if count > 0 {
        let _ = write!(out, "\x1b[{count}F\x1b[0J");
    }
}

fn paint_status_block(
    out: &mut impl Write,
    tasks: &[TaskSpec],
    states: &[TaskState],
    frame: usize,
    start_time: Instant,
    gutter: usize,
    s: &Styles,
) -> usize {
    let mut lines = Vec::new();
    lines.push(String::new()); // blank line separator

    // Header line
    let settled = states.iter().filter(|st| st.phase.is_settled()).count();
    let failed = states.iter().filter(|st| st.phase.is_bad()).count();
    let ratio = if tasks.is_empty() {
        0.0
    } else {
        settled as f64 / tasks.len() as f64
    };
    let width = 24
        .min(terminal_columns().saturating_sub(gutter + 34))
        .max(10);
    let bar_color = if failed > 0 { s.red() } else { s.green() };
    let tally = format!("{settled}/{}", tasks.len());
    let failure_badge = if failed > 0 {
        format!(" {}✖{}{}", s.red(), failed, s.reset())
    } else {
        String::new()
    };
    let elapsed = format_duration(start_time.elapsed());

    lines.push(format!(
        "  {} {}{}{}{} {}{}{}",
        draw_meter(ratio, width, bar_color, s),
        s.bold(),
        tally,
        s.reset(),
        failure_badge,
        s.gray(),
        elapsed,
        s.reset()
    ));

    // Per-task status rows
    for (i, task) in tasks.iter().enumerate() {
        let state = &states[i];
        let icon = phase_icon(state.phase, s, frame);
        let name_styled = if state.phase == Outcome::Pending {
            format!("{}{}{}", s.dim(), task.name.pad_right(gutter), s.reset())
        } else {
            format!(
                "{}{}{}{}",
                s.task_color(task.color_idx),
                s.bold(),
                task.name.pad_right(gutter),
                s.reset()
            )
        };
        let phase_str = phase_text(state.phase, s);
        let time_str = match state.phase {
            Outcome::Running => {
                if let Some(b) = state.began {
                    format_duration(b.elapsed())
                } else {
                    String::new()
                }
            }
            _ => {
                if let Some(d) = state.duration {
                    format_duration(d)
                } else {
                    String::new()
                }
            }
        };

        let row = format!(
            "  {} {} {} {}{}{}",
            icon,
            name_styled,
            phase_str,
            s.gray(),
            time_str.pad_left(8),
            s.reset()
        );
        lines.push(fit_terminal(&row));
    }

    let line_count = lines.len();
    for l in lines {
        let _ = writeln!(out, "{l}");
    }
    line_count
}

fn fit_terminal(line: &str) -> String {
    let max = terminal_columns().saturating_sub(1);
    let vis = visible_width(line);
    if vis <= max {
        line.to_string()
    } else {
        let stripped = strip_ansi(line);
        let truncated: String = stripped.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

fn format_verdict(
    spec: &TaskSpec,
    outcome: Outcome,
    duration: Duration,
    exit_code: i32,
    gutter: usize,
    s: &Styles,
) -> String {
    let icon = match outcome {
        Outcome::Passed => format!("{}✔{}", s.green(), s.reset()),
        Outcome::Failed(_) => format!("{}✖{}", s.red(), s.reset()),
        Outcome::Cancelled => format!("{}⊘{}", s.gray(), s.reset()),
        Outcome::Timeout => format!("{}⏱{}", s.yellow(), s.reset()),
        _ => String::new(),
    };

    let label = format!(
        "{}{}{}{}",
        s.task_color(spec.color_idx),
        s.bold(),
        spec.name.pad_right(gutter),
        s.reset()
    );

    let detail = match outcome {
        Outcome::Passed => format!(
            "{}passed{} {}in {}{}",
            s.green(),
            s.reset(),
            s.gray(),
            format_duration(duration),
            s.reset()
        ),
        Outcome::Failed(_) => format!(
            "{}failed{} {}in {} (exit {}){}",
            s.red(),
            s.reset(),
            s.gray(),
            format_duration(duration),
            exit_code,
            s.reset()
        ),
        Outcome::Cancelled => format!("{}cancelled{}", s.gray(), s.reset()),
        Outcome::Timeout => format!(
            "{}timed out{} {}after {}{}",
            s.yellow(),
            s.reset(),
            s.gray(),
            format_duration(duration),
            s.reset()
        ),
        _ => String::new(),
    };

    format!("{} {} {}\n", icon, label, detail)
}

fn print_plan_header(tasks: &[TaskSpec], opts: &Options, s: &Styles) {
    let filter_scope = opts
        .filter
        .as_ref()
        .map(|f| format!(" --filter {f}"))
        .unwrap_or_default();
    let count_str = format!(
        "{} task{}, concurrent",
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" }
    );
    println!(
        "\n {}{}◆ fanout{} {}{}{}{} {}{}{}",
        s.bold(),
        s.cyan(),
        s.reset(),
        s.bold(),
        opts.target,
        filter_scope,
        s.reset(),
        s.gray(),
        count_str,
        s.reset()
    );

    let pills: Vec<String> = tasks
        .iter()
        .map(|t| {
            format!(
                "{}▪{} {}{}{}",
                s.task_color(t.color_idx),
                s.reset(),
                s.gray(),
                t.name,
                s.reset()
            )
        })
        .collect();

    println!(" {}", pills.join("  "));
    let cols = terminal_columns();
    println!(
        "{}{}{}\n",
        s.gray(),
        "─".repeat(cols.saturating_sub(3)),
        s.reset()
    );
}

fn print_verdict_summary(
    results: &[TaskResult],
    opts: &Options,
    elapsed: Duration,
    gutter: usize,
    s: &Styles,
) {
    let mut ranked = results.to_vec();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.duration));

    let failed: Vec<&TaskResult> = ranked.iter().filter(|r| r.outcome.is_bad()).collect();
    let passed = ranked
        .iter()
        .filter(|r| r.outcome == Outcome::Passed)
        .count();
    let cancelled = ranked.len().saturating_sub(failed.len() + passed);

    let failed_names: String = if !failed.is_empty() {
        format!(
            " — {}",
            failed
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<&str>>()
                .join(", ")
        )
    } else {
        String::new()
    };

    if opts.compact {
        let mut parts = Vec::new();
        parts.push(format!("{passed} passed"));
        if !failed.is_empty() {
            parts.push(format!("{} failed", failed.len()));
        }
        if cancelled > 0 {
            parts.push(format!("{cancelled} cancelled"));
        }
        println!(
            "{} tasks: {} ({}){}",
            results.len(),
            parts.join(", "),
            format_duration(elapsed),
            failed_names
        );
        return;
    }

    let cols = terminal_columns();
    println!(
        "\n{}{}{}",
        s.gray(),
        "─".repeat(cols.saturating_sub(3)),
        s.reset()
    );

    // Summary table with bar meters
    let slowest_ms = ranked
        .iter()
        .map(|r| r.duration.as_millis())
        .max()
        .unwrap_or(1)
        .max(1) as f64;

    let time_col = 8;
    let meter_width = 28.min(cols.saturating_sub(gutter + time_col + 14)).max(8);

    for r in &ranked {
        let icon = match r.outcome {
            Outcome::Passed => format!("{}✔{}", s.green(), s.reset()),
            Outcome::Failed(_) => format!("{}✖{}", s.red(), s.reset()),
            Outcome::Cancelled => format!("{}⊘{}", s.gray(), s.reset()),
            Outcome::Timeout => format!("{}⏱{}", s.yellow(), s.reset()),
            _ => String::new(),
        };
        let label = format!(
            "{}{}{}{}",
            s.task_color(r.color_idx),
            s.bold(),
            r.name.pad_right(gutter),
            s.reset()
        );
        let time_str = format_duration(r.duration);
        let bar = if r.outcome == Outcome::Cancelled {
            String::new()
        } else {
            let ratio = r.duration.as_millis() as f64 / slowest_ms;
            let bar_color = match r.outcome {
                Outcome::Passed => s.green(),
                Outcome::Failed(_) => s.red(),
                Outcome::Timeout => s.yellow(),
                _ => s.gray(),
            };
            draw_meter(ratio, meter_width, bar_color, s)
        };

        println!(
            "  {} {} {}{}{} {}",
            icon,
            label,
            s.gray(),
            time_str.pad_left(time_col),
            s.reset(),
            bar
        );
    }

    println!();
    if failed.is_empty() && cancelled == 0 {
        println!(
            "{}{} ✔ {}/{} passed{} {}in {}{}",
            s.green(),
            s.bold(),
            results.len(),
            results.len(),
            s.reset(),
            s.gray(),
            format_duration(elapsed),
            s.reset()
        );
    } else {
        let cancel_part = if cancelled > 0 {
            format!(", {} cancelled", cancelled)
        } else {
            String::new()
        };
        println!(
            "{}{} ✖ {} failed{}{}, {} passed{} {}in {}{}",
            s.red(),
            s.bold(),
            failed.len(),
            s.reset(),
            cancel_part,
            passed,
            s.reset(),
            s.gray(),
            format_duration(elapsed),
            s.reset()
        );
        if !failed.is_empty() {
            println!(
                "{}   ↳{} {}{}{}",
                s.red(),
                s.reset(),
                s.gray(),
                &failed_names[3..],
                s.reset()
            );
        }
    }
    println!();
}

trait PadExt {
    fn pad_right(&self, width: usize) -> String;
    fn pad_left(&self, width: usize) -> String;
}

impl PadExt for str {
    fn pad_right(&self, width: usize) -> String {
        let w = visible_width(self);
        if w >= width {
            self.to_string()
        } else {
            format!("{}{}", self, " ".repeat(width - w))
        }
    }

    fn pad_left(&self, width: usize) -> String {
        let w = visible_width(self);
        if w >= width {
            self.to_string()
        } else {
            format!("{}{}", " ".repeat(width - w), self)
        }
    }
}
