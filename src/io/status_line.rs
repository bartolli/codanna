//! Status line display with cursor hiding support and ready-to-use progress widgets.
//!
//! Forked from status-line crate (MIT license) with modifications:
//! - Hide cursor during display to prevent visual artifacts on spinners
//! - Show cursor when status line is cleared
//! - Provide concurrency-safe spinner and progress bar helpers with exit-code aware
//!   rendering so CLI callers can surface ✓/✗ outcomes without bespoke plumbing
//!
//! Original: <https://github.com/pkolaczk/status-line>
//!
//! ## Widgets
//! - [`ProgressBar`] renders horizontal bars with configurable glyph palettes so callers
//!   can pick between braille, block, or shaded styles.
//! - [`Spinner`] drives a smooth braille animation that decouples frame rate from work
//!   updates while surfacing ✓/✗ terminal states mapped to [`ExitCode`].
//!
//! Both helpers are thread-safe and fit naturally with [`StatusLine`]'s refresh loop.
//!
//! ## Configuration
//! - Use [`ProgressBarOptions`] to customize glyph style, bar width, and whether rate and
//!   elapsed timing are shown. Options are cheap to copy and can be reused across bars.
//! - [`SpinnerOptions`] exposes the animation frame period (defaults to 100 ms) so callers
//!   can smooth out the braille spinner without tying it to work updates.

use super::ExitCode;
use std::fmt::Display;
use std::io::Write;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::time::Instant;

const DEFAULT_PROGRESS_BAR_WIDTH: usize = 24;

const CURSOR_HIDE: &str = "\x1b[?25l";
const CURSOR_SHOW: &str = "\x1b[?25h";
const ERASE_DOWN: &str = "\x1b[J";
const CURSOR_LEFT: &str = "\r";
const CURSOR_PREV_LINE: &str = "\x1b[1F";

fn redraw(ansi: bool, state: &impl Display) {
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let contents = format!("{state}");
    if ansi {
        let line_count = contents.chars().filter(|c| *c == '\n').count();

        // Hide cursor, erase, write content, move back to start
        write!(
            &mut stderr,
            "{CURSOR_HIDE}{ERASE_DOWN}{contents}{CURSOR_LEFT}"
        )
        .unwrap();

        // Move cursor back to first line, keeping it hidden
        for _ in 0..line_count {
            write!(&mut stderr, "{CURSOR_PREV_LINE}").unwrap();
        }

        // Cursor is now at position 0, line 0, and hidden
        // It stays hidden - no CURSOR_SHOW here
    } else {
        writeln!(&mut stderr, "{contents}").unwrap();
    }
}

fn clear(ansi: bool) {
    if ansi {
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        // Erase and show cursor when clearing
        write!(&mut stderr, "{ERASE_DOWN}{CURSOR_SHOW}").unwrap();
    }
}

struct State<D> {
    data: D,
    visible: AtomicBool,
}

impl<D> State<D> {
    pub fn new(inner: D) -> State<D> {
        State {
            data: inner,
            visible: AtomicBool::new(false),
        }
    }
}

/// Options controlling how to display the status line
pub struct Options {
    /// How long to wait between subsequent refreshes of the status.
    /// Defaults to 100 ms on interactive terminals (TTYs) and 1 s if the standard error
    /// is not interactive, e.g. redirected to a file.
    pub refresh_period: Duration,

    /// Set it to false if you don't want to show the status on creation of the `StatusLine`.
    /// You can change the visibility of the `StatusLine` any time by calling
    /// [`StatusLine::set_visible`].
    pub initially_visible: bool,

    /// Set to true to enable ANSI escape codes.
    /// By default set to true if the standard error is a TTY.
    /// If ANSI escape codes are disabled, the status line is not erased before each refresh,
    /// it is printed in a new line instead.
    pub enable_ansi_escapes: bool,
}

impl Default for Options {
    fn default() -> Self {
        let is_tty = is_terminal::is_terminal(std::io::stderr());
        let refresh_period_ms = if is_tty { 100 } else { 1000 };
        Options {
            refresh_period: Duration::from_millis(refresh_period_ms),
            initially_visible: true,
            enable_ansi_escapes: is_tty,
        }
    }
}

/// Wraps arbitrary data and displays it periodically on the screen.
pub struct StatusLine<D: Display> {
    state: Arc<State<D>>,
    options: Options,
}

impl<D: Display + Send + Sync + 'static> StatusLine<D> {
    /// Creates a new `StatusLine` with default options and shows it immediately.
    pub fn new(data: D) -> StatusLine<D> {
        Self::with_options(data, Default::default())
    }

    /// Creates a new `StatusLine` with custom options.
    pub fn with_options(data: D, options: Options) -> StatusLine<D> {
        let state = Arc::new(State::new(data));
        state
            .visible
            .store(options.initially_visible, Ordering::Release);
        let state_ref = state.clone();
        thread::spawn(move || {
            while Arc::strong_count(&state_ref) > 1 {
                if state_ref.visible.load(Ordering::Acquire) {
                    redraw(options.enable_ansi_escapes, &state_ref.data);
                }
                thread::sleep(options.refresh_period);
            }
        });
        StatusLine { state, options }
    }
}

impl<D: Display> StatusLine<D> {
    /// Forces redrawing the status information immediately,
    /// without waiting for the next refresh cycle of the background refresh loop.
    pub fn refresh(&self) {
        redraw(self.options.enable_ansi_escapes, &self.state.data);
    }

    /// Sets the visibility of the status line.
    pub fn set_visible(&self, visible: bool) {
        let was_visible = self.state.visible.swap(visible, Ordering::Release);
        if !visible && was_visible {
            clear(self.options.enable_ansi_escapes)
        } else if visible && !was_visible {
            redraw(self.options.enable_ansi_escapes, &self.state.data)
        }
    }

    /// Returns true if the status line is currently visible.
    pub fn is_visible(&self) -> bool {
        self.state.visible.load(Ordering::Acquire)
    }
}

impl<D: Display> Deref for StatusLine<D> {
    type Target = D;
    fn deref(&self) -> &Self::Target {
        &self.state.data
    }
}

impl<D: Display> Drop for StatusLine<D> {
    fn drop(&mut self) {
        if self.is_visible() {
            clear(self.options.enable_ansi_escapes)
        }
    }
}

/// Glyph palettes for horizontal progress bars.
#[derive(Clone, Copy, Debug)]
pub enum ProgressBarStyle {
    /// Dense braille wall, good for compact displays.
    Braille,
    /// Full block (`█`) cells with light empty fill (`░`).
    FullBlock,
    /// Dark shade (`▓`) cells with light empty fill (`░`).
    DarkShade,
    /// Medium shade (`▒`) cells on light background.
    MediumShade,
    /// Light shade (`░`) cells with whitespace background.
    LightShade,
    /// Left seven-eighths block (`▉`) for a softer solid look.
    LeftSevenEighths,
    /// Left three-quarters block (`▊`) for medium density bars.
    LeftThreeQuarters,
    /// Left five-eighths block (`▋`) for lighter styling.
    LeftFiveEighths,
    /// Black vertical rectangle (`▮`) that aligns well with bracketed bars.
    VerticalSolid,
    /// White vertical rectangle (`▯`) complement for `VerticalSolid`.
    VerticalLight,
    /// Black parallelogram (`▰`) for a stylized slanted bar.
    ParallelogramSolid,
    /// White parallelogram (`▱`) complement for `ParallelogramSolid`.
    ParallelogramLight,
}

impl ProgressBarStyle {
    /// Glyph used for filled segments of the bar.
    pub fn filled_cell(self) -> &'static str {
        match self {
            ProgressBarStyle::Braille => "⣿",
            ProgressBarStyle::FullBlock => "█",
            ProgressBarStyle::DarkShade => "▓",
            ProgressBarStyle::MediumShade => "▒",
            ProgressBarStyle::LightShade => "░",
            ProgressBarStyle::LeftSevenEighths => "▉",
            ProgressBarStyle::LeftThreeQuarters => "▊",
            ProgressBarStyle::LeftFiveEighths => "▋",
            ProgressBarStyle::VerticalSolid => "▮",
            ProgressBarStyle::VerticalLight => "▯",
            ProgressBarStyle::ParallelogramSolid => "▰",
            ProgressBarStyle::ParallelogramLight => "▱",
        }
    }

    /// Glyph used for empty segments of the bar.
    pub fn empty_cell(self) -> &'static str {
        match self {
            ProgressBarStyle::Braille => " ",
            ProgressBarStyle::FullBlock
            | ProgressBarStyle::DarkShade
            | ProgressBarStyle::MediumShade => "░",
            ProgressBarStyle::LightShade => " ",
            ProgressBarStyle::LeftSevenEighths
            | ProgressBarStyle::LeftThreeQuarters
            | ProgressBarStyle::LeftFiveEighths => "░",
            ProgressBarStyle::VerticalSolid => " ",
            ProgressBarStyle::ParallelogramSolid => " ",
            ProgressBarStyle::VerticalLight => " ",
            ProgressBarStyle::ParallelogramLight => " ",
        }
    }
}

/// Configuration options for [`ProgressBar`].
#[derive(Clone, Copy, Debug)]
pub struct ProgressBarOptions {
    pub style: ProgressBarStyle,
    pub width: usize,
    pub show_rate: bool,
    pub show_elapsed: bool,
}

impl ProgressBarOptions {
    pub fn new(style: ProgressBarStyle, width: usize) -> Self {
        Self {
            style,
            width: width.max(1),
            show_rate: true,
            show_elapsed: true,
        }
    }

    pub fn with_style(mut self, style: ProgressBarStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width.max(1);
        self
    }

    pub fn show_rate(mut self, show: bool) -> Self {
        self.show_rate = show;
        self
    }

    pub fn show_elapsed(mut self, show: bool) -> Self {
        self.show_elapsed = show;
        self
    }
}

impl Default for ProgressBarOptions {
    fn default() -> Self {
        Self::new(ProgressBarStyle::FullBlock, DEFAULT_PROGRESS_BAR_WIDTH)
    }
}

/// Minimal-allocation progress bar that can be rendered through [`StatusLine`].
///
/// Uses interior atomics so it can be shared across threads without locks.
pub struct ProgressBar {
    current: AtomicU64,
    total: u64,
    extra1: AtomicU64,
    extra2: AtomicU64,
    extra3: AtomicU64,
    labels: (&'static str, &'static str, &'static str, &'static str),
    start_time: Instant,
    options: ProgressBarOptions,
}

impl ProgressBar {
    /// Create a progress bar with the provided total count and label.
    pub fn new(total: u64, label: &'static str) -> Self {
        Self::with_options(total, label, "", "", ProgressBarOptions::default())
    }

    /// Create a progress bar with extra counters and custom options.
    pub fn with_options(
        total: u64,
        label: &'static str,
        extra1_label: &'static str,
        extra2_label: &'static str,
        mut options: ProgressBarOptions,
    ) -> Self {
        options.width = options.width.max(1);
        Self {
            current: AtomicU64::new(0),
            total,
            extra1: AtomicU64::new(0),
            extra2: AtomicU64::new(0),
            extra3: AtomicU64::new(0),
            labels: (label, extra1_label, extra2_label, ""),
            start_time: Instant::now(),
            options,
        }
    }

    /// Create a progress bar with 4 labels (main + 3 extra counters).
    pub fn with_4_labels(
        total: u64,
        label: &'static str,
        extra1_label: &'static str,
        extra2_label: &'static str,
        extra3_label: &'static str,
        mut options: ProgressBarOptions,
    ) -> Self {
        options.width = options.width.max(1);
        Self {
            current: AtomicU64::new(0),
            total,
            extra1: AtomicU64::new(0),
            extra2: AtomicU64::new(0),
            extra3: AtomicU64::new(0),
            labels: (label, extra1_label, extra2_label, extra3_label),
            start_time: Instant::now(),
            options,
        }
    }

    /// Create a progress bar with extra counters and quick style override.
    pub fn with_extras(
        total: u64,
        label: &'static str,
        extra1_label: &'static str,
        extra2_label: &'static str,
        style: ProgressBarStyle,
    ) -> Self {
        Self::with_options(
            total,
            label,
            extra1_label,
            extra2_label,
            ProgressBarOptions::default().with_style(style),
        )
    }

    /// Update options (width is clamped to at least 1 cell).
    pub fn set_options(&mut self, mut options: ProgressBarOptions) {
        options.width = options.width.max(1);
        self.options = options;
    }

    /// Convenience setter for the visual width.
    pub fn set_width(&mut self, width: usize) {
        self.options.width = width.max(1);
    }

    /// Increment the main counter by one.
    pub fn inc(&self) {
        self.current.fetch_add(1, Ordering::Relaxed);
    }

    /// Set the main counter to a specific value.
    pub fn set_progress(&self, value: u64) {
        self.current.store(value, Ordering::Relaxed);
    }

    /// Get the current progress value.
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    /// Increase the first auxiliary counter.
    pub fn add_extra1(&self, n: u64) {
        self.extra1.fetch_add(n, Ordering::Relaxed);
    }

    /// Increase the second auxiliary counter.
    pub fn add_extra2(&self, n: u64) {
        self.extra2.fetch_add(n, Ordering::Relaxed);
    }

    /// Increase the third auxiliary counter.
    pub fn add_extra3(&self, n: u64) {
        self.extra3.fetch_add(n, Ordering::Relaxed);
    }

    /// Update the total expected count (resets elapsed timer).
    pub fn reset_total(&mut self, total: u64) {
        self.total = total;
        self.current.store(0, Ordering::Relaxed);
        self.extra1.store(0, Ordering::Relaxed);
        self.extra2.store(0, Ordering::Relaxed);
        self.extra3.store(0, Ordering::Relaxed);
        self.start_time = Instant::now();
    }
}

impl Display for ProgressBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = self.current.load(Ordering::Relaxed);
        let extra1 = self.extra1.load(Ordering::Relaxed);
        let extra2 = self.extra2.load(Ordering::Relaxed);
        let extra3 = self.extra3.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed().as_secs_f64();

        let ratio = if self.total > 0 {
            (current as f64 / self.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pct = (ratio * 100.0).round() as u8;
        let filled = (ratio * self.options.width as f64).round() as usize;
        let filled = filled.min(self.options.width);
        let empty = self.options.width - filled;
        let bar = format!(
            "{}{}",
            self.options.style.filled_cell().repeat(filled),
            self.options.style.empty_cell().repeat(empty)
        );

        let rate = if elapsed > 0.0 {
            current as f64 / elapsed
        } else {
            0.0
        };

        writeln!(f, "Progress: [{bar}] {pct:3}%")?;
        write!(f, "{}/{} {}", current, self.total, self.labels.0)?;

        if !self.labels.1.is_empty() && extra1 > 0 {
            write!(f, " | {} {}", extra1, self.labels.1)?;
        }

        if !self.labels.2.is_empty() && extra2 > 0 {
            write!(f, " | {} {}", extra2, self.labels.2)?;
        }

        if !self.labels.3.is_empty() && extra3 > 0 {
            write!(f, " | {} {}", extra3, self.labels.3)?;
        }

        if self.options.show_rate {
            write!(f, " | {rate:.0}/s")?;
        }

        if self.options.show_elapsed {
            write!(f, " | {elapsed:.1}s")?;
        }

        Ok(())
    }
}

/// Configuration options for [`Spinner`].
#[derive(Clone, Copy, Debug)]
pub struct SpinnerOptions {
    pub frame_period: Duration,
}

impl SpinnerOptions {
    pub fn new(frame_period: Duration) -> Self {
        let frame_period = if frame_period.as_millis() == 0 {
            Duration::from_millis(1)
        } else {
            frame_period
        };
        Self { frame_period }
    }

    pub fn with_frame_period(mut self, frame_period: Duration) -> Self {
        self.frame_period = if frame_period.as_millis() == 0 {
            Duration::from_millis(1)
        } else {
            frame_period
        };
        self
    }
}

impl Default for SpinnerOptions {
    fn default() -> Self {
        Self::new(Duration::from_millis(100))
    }
}

/// Animation state for streaming operations with exit-code reporting.
pub struct Spinner {
    count: AtomicU64,
    extra1: AtomicU64,
    label: &'static str,
    extra_label: &'static str,
    start_time: Instant,
    frame_period_ms: AtomicU64,
    outcome_state: AtomicU8,
    exit_code: AtomicU8,
    error_message: Mutex<Option<String>>,
}

impl Spinner {
    const FRAMES: &'static [&'static str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    const STATE_PENDING: u8 = 0;
    const STATE_PREPARING_SUCCESS: u8 = 1;
    const STATE_PREPARING_FAILURE: u8 = 2;
    const STATE_SUCCESS: u8 = 3;
    const STATE_FAILURE: u8 = 4;

    /// Construct a spinner with the provided label and default options.
    pub fn new(label: &'static str, extra_label: &'static str) -> Self {
        Self::with_options(label, extra_label, SpinnerOptions::default())
    }

    /// Construct a spinner with custom options.
    pub fn with_options(
        label: &'static str,
        extra_label: &'static str,
        options: SpinnerOptions,
    ) -> Self {
        let frame_period_ms = options.frame_period.as_millis().max(1) as u64;
        Self {
            count: AtomicU64::new(0),
            extra1: AtomicU64::new(0),
            label,
            extra_label,
            start_time: Instant::now(),
            frame_period_ms: AtomicU64::new(frame_period_ms),
            outcome_state: AtomicU8::new(Self::STATE_PENDING),
            exit_code: AtomicU8::new(ExitCode::Success as u8),
            error_message: Mutex::new(None),
        }
    }

    /// Convenience constructor to override only the animation period.
    pub fn with_frame_period(
        label: &'static str,
        extra_label: &'static str,
        frame_period: Duration,
    ) -> Self {
        Self::with_options(
            label,
            extra_label,
            SpinnerOptions::default().with_frame_period(frame_period),
        )
    }

    /// Adjust the options on the fly.
    pub fn set_options(&self, options: SpinnerOptions) {
        self.frame_period_ms.store(
            options.frame_period.as_millis().max(1) as u64,
            Ordering::Relaxed,
        );
    }

    /// Increment the primary counter and advance animation.
    pub fn tick(&self) {
        if self.is_finished() {
            return;
        }
        self.count.fetch_add(1, Ordering::Relaxed);

        if self.is_finished() {
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Add to the auxiliary counter (e.g. batches processed).
    pub fn add_extra(&self, n: u64) {
        if self.is_finished() {
            return;
        }

        self.extra1.fetch_add(n, Ordering::Relaxed);

        if self.is_finished() {
            self.extra1.fetch_sub(n, Ordering::Relaxed);
        }
    }

    /// Mark the spinner as succeeded, switching to ✓ output.
    pub fn mark_success(&self) {
        if self
            .outcome_state
            .compare_exchange(
                Self::STATE_PENDING,
                Self::STATE_PREPARING_SUCCESS,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.exit_code
                .store(ExitCode::Success as u8, Ordering::Relaxed);
            let mut message = self
                .error_message
                .lock()
                .expect("spinner error message mutex poisoned");
            *message = None;
            drop(message);
            self.outcome_state
                .store(Self::STATE_SUCCESS, Ordering::Release);
        }
    }

    /// Mark the spinner as failed, providing an exit code and optional message.
    pub fn mark_failure(&self, code: ExitCode, message: impl Into<String>) {
        if self
            .outcome_state
            .compare_exchange(
                Self::STATE_PENDING,
                Self::STATE_PREPARING_FAILURE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.exit_code.store(code as u8, Ordering::Relaxed);
            let mut stored = self
                .error_message
                .lock()
                .expect("spinner error message mutex poisoned");
            *stored = Some(message.into());
            drop(stored);
            self.outcome_state
                .store(Self::STATE_FAILURE, Ordering::Release);
        }
    }

    /// Check whether the spinner reached a terminal state.
    pub fn is_finished(&self) -> bool {
        !matches!(
            self.outcome_state.load(Ordering::Acquire),
            Self::STATE_PENDING
        )
    }

    /// Retrieve the currently published exit code.
    pub fn current_exit_code(&self) -> ExitCode {
        match self.exit_code.load(Ordering::Acquire) {
            0 => ExitCode::Success,
            1 => ExitCode::GeneralError,
            2 => ExitCode::BlockingError,
            3 => ExitCode::NotFound,
            4 => ExitCode::ParseError,
            5 => ExitCode::IoError,
            6 => ExitCode::ConfigError,
            7 => ExitCode::IndexCorrupted,
            8 => ExitCode::UnsupportedOperation,
            _ => ExitCode::GeneralError,
        }
    }
}

impl Display for Spinner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.count.load(Ordering::Relaxed);
        let extra1 = self.extra1.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 {
            count as f64 / elapsed
        } else {
            0.0
        };

        let frame_period_ms = self.frame_period_ms.load(Ordering::Relaxed).max(1);
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        let state = self.outcome_state.load(Ordering::Acquire);
        let frame_idx = match state {
            Self::STATE_PENDING => (elapsed_ms / frame_period_ms) as usize % Self::FRAMES.len(),
            Self::STATE_PREPARING_SUCCESS | Self::STATE_SUCCESS => 0,
            Self::STATE_PREPARING_FAILURE | Self::STATE_FAILURE => Self::FRAMES.len() - 1,
            _ => unreachable!("invalid spinner state"),
        };
        let spinner = Self::FRAMES[frame_idx % Self::FRAMES.len()];

        let mut line = match state {
            Self::STATE_PENDING => {
                let mut pending = format!("{spinner} {} | {} items", self.label, count);
                if !self.extra_label.is_empty() && extra1 > 0 {
                    pending.push_str(&format!(" | {} {}", extra1, self.extra_label));
                }
                pending
            }
            Self::STATE_PREPARING_SUCCESS | Self::STATE_SUCCESS => {
                let mut success = format!("✓ {} complete | {} items", self.label, count);
                if !self.extra_label.is_empty() && extra1 > 0 {
                    success.push_str(&format!(" | {} {}", extra1, self.extra_label));
                }
                success
            }
            Self::STATE_PREPARING_FAILURE | Self::STATE_FAILURE => {
                let code = self.current_exit_code();
                let mut failure = format!(
                    "✗ {} failed [exit code {} - {}]",
                    self.label,
                    code as u8,
                    code.description()
                );
                if let Some(message) = self
                    .error_message
                    .lock()
                    .expect("spinner error message mutex poisoned")
                    .clone()
                {
                    failure.push_str(&format!(": {message}"));
                }
                failure.push_str(&format!(" | processed {count}"));
                if !self.extra_label.is_empty() && extra1 > 0 {
                    failure.push_str(&format!(" | {} {}", extra1, self.extra_label));
                }
                failure
            }
            _ => unreachable!("invalid spinner state"),
        };

        line.push_str(&format!(" | {rate:.0}/s | {elapsed:.1}s"));
        write!(f, "{line}")
    }
}
