//! `ttui` command-line entry point.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ttui::VERSION;
use ttui::app::App;
use ttui::app::state::AppState;
use ttui::platform::paths::PlatformPaths;
use ttui::platform::shell::{command_exists, detect_shell};
use ttui::providers::CredentialStore;
use ttui::ui::theme::ColorDepth;
use ttui::ui::{TerminalSession, UiRenderer};

const HELP: &str = "\
ttui — TradinationalTUI, a terminal IDE

USAGE:
    ttui [OPTIONS] [PATH]

ARGS:
    PATH    Directory to open as the workspace, or a file to open

OPTIONS:
    -h, --help                 Print this help
    -V, --version              Print version
        --check-health         Print environment diagnostics
        --headless-smoke-test  Render one frame without a TTY

KEYS:
    i / Esc        insert / normal mode
    Ctrl+Space     command palette
    Ctrl+P         go to file
    Ctrl+T         open terminal (Ctrl+Space to leave shell)
    Ctrl+G         connect an ACP agent
    Ctrl+O         open configuration
    Ctrl+Shift+F   project search
    Ctrl+F         buffer search
    Ctrl+B         explorer
    Ctrl+Alt+B     AI sidebar
    Ctrl+Q         quit (twice when dirty)
    F1             keymap help
";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_text(HELP)?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        print_text(&format!("ttui {VERSION}"))?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--check-health") {
        return check_health();
    }
    if args.iter().any(|arg| arg == "--headless-smoke-test") {
        return headless_smoke_test();
    }

    let _log_guard = init_logging();
    ttui::ui::session::install_panic_hook();

    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let (root, initial_file) = resolve_paths(&args, &cwd)?;
    let state = AppState::bootstrap(&root, initial_file.as_deref())?;
    let mouse = state.config.ui.mouse;

    let mut session = TerminalSession::enter(mouse).context("failed to initialize terminal")?;
    let result = App::new(state).run(session.terminal_mut()).await;
    session.restore();
    result?;
    Ok(())
}

fn resolve_paths(args: &[String], cwd: &std::path::Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if let Some(flag) = args.iter().find(|arg| arg.starts_with('-')) {
        bail!("unknown option: {flag}");
    }
    let Some(positional) = args.first() else {
        return Ok((cwd.to_path_buf(), None));
    };
    let path = PathBuf::from(positional);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    if path.is_dir() {
        Ok((path, None))
    } else if path.is_file() {
        Ok((cwd.to_path_buf(), Some(path)))
    } else {
        bail!("path does not exist: {}", path.display())
    }
}

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let paths = PlatformPaths::discover();
    if paths.ensure_dirs().is_err() {
        return None;
    }
    let appender = tracing_appender::rolling::never(paths.log_dir(), "ttui.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("ttui=info"));
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .with_env_filter(filter)
        .try_init();
    Some(guard)
}

fn check_health() -> Result<()> {
    use std::fmt::Write as _;
    let paths = PlatformPaths::discover();
    let cwd = std::env::current_dir()?;
    let config = ttui::config::load(&cwd).unwrap_or_default();
    let shell = detect_shell(config.terminal.shell.as_deref());
    let depth = match ColorDepth::detect(config.ui.truecolor) {
        ColorDepth::TrueColor => "truecolor",
        ColorDepth::Ansi256 => "256-color",
    };
    let agents = ttui::acp::registry::discover(&config.acp.agents);
    let available: Vec<&str> = agents
        .iter()
        .filter(|profile| profile.available)
        .map(|profile| profile.id.as_str())
        .collect();
    let provider = config
        .ai
        .default_provider
        .clone()
        .unwrap_or_else(|| "disabled".to_string());
    let has_key = config
        .ai
        .default_provider
        .as_deref()
        .is_some_and(|id| CredentialStore::new().has_key(id));

    let mut output = String::new();
    let _ = writeln!(output, "ttui {VERSION}");
    let _ = writeln!(output, "config dir: {}", paths.config_dir().display());
    let _ = writeln!(output, "data dir:   {}", paths.data_dir().display());
    let _ = writeln!(output, "state dir:  {}", paths.state_dir().display());
    let _ = writeln!(output, "log dir:    {}", paths.log_dir().display());
    let _ = writeln!(output, "shell:      {}", shell.display());
    let _ = writeln!(
        output,
        "git:        {}",
        if command_exists("git") {
            "installed"
        } else {
            "missing"
        }
    );
    let _ = writeln!(output, "color:      {depth}");
    let _ = writeln!(
        output,
        "acp agents: {}",
        if available.is_empty() {
            "none installed".to_string()
        } else {
            available.join(", ")
        }
    );
    let _ = writeln!(
        output,
        "ai:         {provider} ({})",
        if has_key { "key present" } else { "no key" }
    );
    write_stdout(&output)
}

fn headless_smoke_test() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("ttui-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let painted = (|| -> Result<usize> {
        let state = AppState::bootstrap(&dir, None)?;
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend)?;
        let renderer = UiRenderer::new();
        terminal.draw(|frame| renderer.render(frame, &state))?;
        let buffer = terminal.backend().buffer();
        let mut painted = 0;
        for y in 0..40 {
            for x in 0..120 {
                if buffer
                    .cell((x, y))
                    .is_some_and(|cell| !cell.symbol().trim().is_empty())
                {
                    painted += 1;
                }
            }
        }
        Ok(painted)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let painted = painted?;
    write_stdout(&format!("headless smoke: ok ({painted} painted cells)\n"))
}

fn print_text(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{text}")?;
    stdout.flush()?;
    Ok(())
}

fn write_stdout(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_paths_defaults_to_cwd() {
        let cwd = std::env::current_dir().expect("cwd");
        let (root, file) = resolve_paths(&[], &cwd).expect("resolve");
        assert_eq!(root, cwd);
        assert!(file.is_none());
    }

    #[test]
    fn resolve_paths_handles_directories() {
        let cwd = std::env::current_dir().expect("cwd");
        let args = vec![cwd.display().to_string()];
        let (root, file) = resolve_paths(&args, &cwd).expect("resolve");
        assert_eq!(root, cwd);
        assert!(file.is_none());
    }

    #[test]
    fn resolve_paths_rejects_missing_paths() {
        let cwd = std::env::current_dir().expect("cwd");
        let args = vec!["definitely-missing-ttui-path".to_string()];
        assert!(resolve_paths(&args, &cwd).is_err());
    }
}
