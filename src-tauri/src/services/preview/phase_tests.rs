use super::*;

const SUCCESS: &str = include_str!("fixtures/tauri_dev_success.log");
const ERROR: &str = include_str!("fixtures/tauri_dev_build_error.log");

/// Phases in order of change, driving the parser with a recorded `tauri dev` log the way
/// the service does (split on \n, clean each line).
fn run(log: &str) -> (Vec<PreviewPhase>, PhaseParser) {
    let mut p = PhaseParser::new(PreviewPhase::Compiling);
    let mut seen = Vec::new();
    for raw in log.split('\n') {
        let line = clean_line(raw);
        if let Some(ph) = p.feed(&line) {
            seen.push(ph.clone());
        }
    }
    (seen, p)
}

#[test]
fn recorded_successful_run_ends_in_app_running() {
    let (seen, p) = run(SUCCESS);
    assert_eq!(seen, [PreviewPhase::AppRunning]);
    assert_eq!(p.phase_for_test(), &PreviewPhase::AppRunning);
}

#[test]
fn recorded_rust_error_is_build_failed_with_the_first_error_line() {
    let (seen, p) = run(ERROR);
    // The specific error wins over cargo's "could not compile" summary.
    let want = PreviewPhase::BuildFailed {
        first_error: Some("error[E0308]: mismatched types".into()),
    };
    assert_eq!(seen.last(), Some(&want), "{seen:?}");
    assert_eq!(p.phase_for_test(), &want);
    assert_eq!(p.first_error(), Some("error[E0308]: mismatched types"));
}

#[test]
fn a_new_build_after_a_failure_clears_the_error_and_success_means_app_running() {
    let (_, mut p) = run(ERROR);
    assert_eq!(
        p.feed("Compiling ai-app-studio v0.1.0 (/p)"),
        Some(&PreviewPhase::Compiling)
    );
    assert_eq!(p.first_error(), None);
    assert_eq!(
        p.feed("Running `target/debug/ai-app-studio`"),
        Some(&PreviewPhase::AppRunning)
    );
}

#[test]
fn a_summary_line_is_used_only_until_something_more_specific_arrives() {
    let mut p = PhaseParser::new(PreviewPhase::Compiling);
    p.feed("error: could not compile `x` (lib) due to 1 previous error");
    assert_eq!(
        p.first_error(),
        Some("error: could not compile `x` (lib) due to 1 previous error")
    );
    p.feed("error[E0425]: cannot find value `y` in this scope");
    assert_eq!(
        p.first_error(),
        Some("error[E0425]: cannot find value `y` in this scope")
    );
    // Later errors don't replace the first.
    p.feed("error[E0599]: no method named `z`");
    assert_eq!(
        p.first_error(),
        Some("error[E0425]: cannot find value `y` in this scope")
    );
}

#[test]
fn waiting_for_another_rust_build() {
    let mut p = PhaseParser::new(PreviewPhase::Compiling);
    assert_eq!(
        p.feed("    Blocking waiting for file lock on build directory"),
        Some(&PreviewPhase::WaitingForRustBuild)
    );
    assert_eq!(
        p.feed("    Blocking waiting for file lock on package cache"),
        None,
        "the same phase is no change"
    );
    assert_eq!(
        p.feed("   Compiling foo v1.0.0"),
        Some(&PreviewPhase::Compiling)
    );
}

#[test]
fn unknown_output_keeps_the_last_phase() {
    let mut p = PhaseParser::new(PreviewPhase::Installing);
    for line in [
        "  VITE v8.3.5  ready in 151 ms",
        "warning: unused variable: `x`",
        "",
        "hello",
        "some error: not at the start",
    ] {
        assert_eq!(p.feed(line), None, "{line}");
    }
    assert_eq!(p.phase_for_test(), &PreviewPhase::Installing);
}

// Hand-written from Vite's known wording: on the recording machine a taken port made Vite
// bind another address instead of failing, so no real failing log could be captured.
#[test]
fn port_in_use_is_noted() {
    let mut p = PhaseParser::new(PreviewPhase::Compiling);
    assert_eq!(p.port_in_use(), None);
    p.feed("Error: Port 1420 is already in use");
    assert_eq!(p.port_in_use(), Some(1420));
}

#[test]
fn clean_line_strips_ansi_and_keeps_the_last_redraw() {
    assert_eq!(
        clean_line("\x1b[K\x1b[1m\x1b[91merror[E0308]\x1b[0m\x1b[1m: mismatched types\x1b[0m"),
        "error[E0308]: mismatched types"
    );
    assert_eq!(
        clean_line("Building [=>] 1/9\r\x1b[K    Finished `dev`"),
        "    Finished `dev`"
    );
    assert_eq!(clean_line("\r"), "");
}
