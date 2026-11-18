//! Command line (PLAN section 5.1).

/// What the process should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Run the background daemon (default).
    Daemon,
    /// Show the settings window.
    Settings,
    /// Join a group from a pairing URI.
    Pair(String),
    /// Ask the running daemon to quit.
    Quit,
    /// Send these files or folders to all online devices.
    Send(Vec<String>),
    /// Pick the target device(s) in a small window, then send (used by the SendTo shortcut).
    SendUi(Vec<String>),
}

/// Parsed arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    /// Selected mode.
    pub mode: Mode,
    /// Overrides `log_level` from the config.
    pub log_level: Option<String>,
    /// Debug aid: exit the daemon after this many seconds.
    pub hold_secs: Option<u64>,
}

/// Parse arguments (without the program name).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli {
        mode: Mode::Daemon,
        log_level: None,
        hold_secs: None,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--settings" => cli.mode = Mode::Settings,
            "--quit" => cli.mode = Mode::Quit,
            "--send" => {
                let paths: Vec<String> = it.by_ref().collect();
                if paths.is_empty() {
                    return Err("--send needs at least one file or folder".into());
                }
                cli.mode = Mode::Send(paths);
            }
            "--send-ui" => {
                let paths: Vec<String> = it.by_ref().collect();
                if paths.is_empty() {
                    return Err("--send-ui needs at least one file or folder".into());
                }
                cli.mode = Mode::SendUi(paths);
            }
            "--pair" => {
                let uri = it.next().ok_or("--pair needs a transclip:// URI")?;
                cli.mode = Mode::Pair(uri);
            }
            "--log-level" => {
                let lvl = it.next().ok_or("--log-level needs a value")?;
                if !crate::config::LOG_LEVELS.contains(&lvl.as_str()) {
                    return Err(format!("unknown log level: {lvl}"));
                }
                cli.log_level = Some(lvl);
            }
            "--hold-secs" => {
                let v = it.next().ok_or("--hold-secs needs a number")?;
                cli.hold_secs = Some(v.parse().map_err(|_| "--hold-secs needs a number")?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cli, String> {
        parse(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn send_ui_collects_the_paths() {
        let c = p(&["--send-ui", "a.txt", "b"]).unwrap();
        assert_eq!(c.mode, Mode::SendUi(vec!["a.txt".into(), "b".into()]));
        assert!(p(&["--send-ui"]).is_err());
    }

    #[test]
    fn defaults_to_daemon() {
        assert_eq!(p(&[]).unwrap().mode, Mode::Daemon);
    }

    #[test]
    fn modes_and_options() {
        assert_eq!(p(&["--settings"]).unwrap().mode, Mode::Settings);
        assert_eq!(p(&["--quit"]).unwrap().mode, Mode::Quit);
        assert_eq!(
            p(&["--send", "a.txt", "b c.txt"]).unwrap().mode,
            Mode::Send(vec!["a.txt".into(), "b c.txt".into()])
        );
        assert_eq!(
            p(&["--pair", "transclip://pair?v=1"]).unwrap().mode,
            Mode::Pair("transclip://pair?v=1".into())
        );
        assert_eq!(
            p(&["--log-level", "debug"]).unwrap().log_level.as_deref(),
            Some("debug")
        );
        assert_eq!(p(&["--hold-secs", "3"]).unwrap().hold_secs, Some(3));
    }

    #[test]
    fn bad_input_is_reported() {
        assert!(p(&["--pair"]).is_err());
        assert!(p(&["--send"]).is_err());
        assert!(p(&["--log-level", "loud"]).is_err());
        assert!(p(&["--bogus"]).is_err());
        assert!(p(&["--hold-secs", "x"]).is_err());
    }
}
