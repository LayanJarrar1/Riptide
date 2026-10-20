//! Post-queue actions: what happens after the webview's countdown ran out.
//!
//! The engine only *announces* a finished queue (`Event::QueueFinished`); the webview shows the
//! 30-second countdown and then calls [`crate::queues::run_post_action`] with the **queue id**.
//! The action itself is read back from the engine's saved queue, so the webview cannot make
//! this process run anything the user did not save in the queue's settings.
//!
//! Everything platform-specific sits behind the [`Power`] trait, and the Windows implementation
//! lives in the small `sys` module at the bottom of this file. Tests drive [`perform`] with a
//! fake [`Power`] and never touch the machine.
//!
//! | Action | Windows |
//! |---|---|
//! | Exit | `AppHandle::exit` (after the transfers stopped at a checkpoint) |
//! | Sleep / Hibernate | `SetSuspendState` |
//! | Shut down | `%SystemRoot%\System32\shutdown.exe /s /t 0` (no elevation for the interactive user) |
//! | Run a command | the saved program path, **no arguments**, never through a shell |
//!
//! Other platforms answer "not supported" for everything but Exit and Run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use riptide_core::PostAction;

/// Arguments of `shutdown.exe` for an immediate shutdown. Fixed: nothing user-supplied.
pub const SHUTDOWN_ARGS: [&str; 3] = ["/s", "/t", "0"];

/// Extensions Windows runs through `cmd.exe`; refused because the action never uses a shell.
const SHELL_SCRIPTS: [&str; 2] = ["bat", "cmd"];

/// The one process a "Run a command" action may start: the saved program and a fixed argument
/// list (empty today). Built only by [`build_run_spec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    /// Absolute path of an existing program.
    pub program: PathBuf,
    /// Fixed arguments; never derived from user text.
    pub args: Vec<OsString>,
}

/// Turns the command the user saved into a [`RunSpec`], or says why it cannot run.
///
/// The whole text is one program path (surrounding double quotes are dropped, nothing is split
/// on spaces, no arguments exist): it must be absolute, free of control characters, an
/// existing file, and not a `.bat`/`.cmd` script (those start `cmd.exe`).
pub fn build_run_spec(command: &str) -> Result<RunSpec, String> {
    let text = command.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .trim();
    if text.is_empty() {
        return Err("no program is set".into());
    }
    if text.chars().any(char::is_control) {
        return Err("the program path has control characters".into());
    }
    let program = Path::new(text);
    if !program.is_absolute() {
        return Err("the program must be a full path (for example C:\\Tools\\done.exe)".into());
    }
    // Windows drops trailing dots and spaces from the last component, so `run.bat.` and
    // `run.bat .` start `run.bat`: look at the name the way Windows will
    let normalized = text.trim_end_matches(['.', ' ']);
    let is_script = Path::new(normalized)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SHELL_SCRIPTS.iter().any(|s| e.eq_ignore_ascii_case(s)));
    if is_script {
        return Err("scripts (.bat, .cmd) are not run; choose a program (.exe)".into());
    }
    match std::fs::metadata(program) {
        Ok(m) if m.is_file() => Ok(RunSpec {
            program: program.to_path_buf(),
            args: Vec::new(),
        }),
        Ok(_) => Err("the program path is not a file".into()),
        Err(_) => Err("the program does not exist".into()),
    }
}

/// Checks that `action` could run now; used when a queue is saved so a typo shows up then and
/// not at 3 a.m.
pub fn validate(action: &PostAction) -> Result<(), String> {
    match action {
        PostAction::RunCommand { command } => build_run_spec(command).map(|_| ()),
        _ => Ok(()),
    }
}

/// [`validate`] for the action of a queue spec (nothing to check without one).
pub fn validate_spec(spec: &riptide_core::QueueSpec) -> Result<(), String> {
    spec.on_done.as_ref().map_or(Ok(()), validate)
}

/// Whether the transfers must be stopped at a checkpoint before `action` runs.
pub fn stops_engine(action: &PostAction) -> bool {
    matches!(action, PostAction::Exit)
}

/// What the machine can be asked to do. [`SystemPower`] is the real one.
pub trait Power {
    /// Suspend to RAM (`hibernate == false`) or to disk.
    fn suspend(&self, hibernate: bool) -> Result<(), String>;
    /// Shut the computer down now.
    fn shutdown(&self) -> Result<(), String>;
    /// Start the program of a validated [`RunSpec`] and do not wait for it.
    fn run(&self, spec: &RunSpec) -> Result<(), String>;
    /// Quit Riptide.
    fn exit(&self);
}

/// Performs `action` through `power`.
pub fn perform(power: &dyn Power, action: &PostAction) -> Result<(), String> {
    match action {
        PostAction::Exit => {
            power.exit();
            Ok(())
        }
        PostAction::Sleep => power.suspend(false),
        PostAction::Hibernate => power.suspend(true),
        PostAction::Shutdown => power.shutdown(),
        PostAction::RunCommand { command } => power.run(&build_run_spec(command)?),
    }
}

/// The real machine.
pub struct SystemPower {
    app: tauri::AppHandle,
}

impl SystemPower {
    /// Power control that quits `app` for [`PostAction::Exit`].
    pub fn new(app: tauri::AppHandle) -> Self {
        SystemPower { app }
    }
}

impl Power for SystemPower {
    fn suspend(&self, hibernate: bool) -> Result<(), String> {
        sys::suspend(hibernate)
    }

    fn shutdown(&self) -> Result<(), String> {
        sys::shutdown()
    }

    fn run(&self, spec: &RunSpec) -> Result<(), String> {
        // direct process creation: no shell, no arguments from the user, output discarded
        std::process::Command::new(&spec.program)
            .args(&spec.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_child| ())
            .map_err(|e| format!("could not start the program: {e}"))
    }

    fn exit(&self) {
        self.app.exit(0);
    }
}

/// Windows: `SetSuspendState` and `shutdown.exe`. The only `unsafe` in this crate.
#[cfg(windows)]
mod sys {
    #![allow(unsafe_code)]

    use std::path::PathBuf;

    use windows_sys::Win32::System::Power::SetSuspendState;

    pub(super) fn suspend(hibernate: bool) -> Result<(), String> {
        // SAFETY: plain value arguments (hibernate, no force, wake events enabled); the call
        // touches no memory of ours.
        let ok = unsafe { SetSuspendState(u8::from(hibernate), 0, 0) };
        if ok == 0 {
            Err(if hibernate {
                "the computer could not hibernate (is hibernation turned on?)".into()
            } else {
                "the computer could not go to sleep".into()
            })
        } else {
            Ok(())
        }
    }

    pub(super) fn shutdown() -> Result<(), String> {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = PathBuf::from(system).join("System32").join("shutdown.exe");
        let status = std::process::Command::new(exe)
            .args(super::SHUTDOWN_ARGS)
            .status()
            .map_err(|e| format!("could not run shutdown: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("shutdown failed ({status})"))
        }
    }
}

/// Everywhere else: not supported yet.
#[cfg(not(windows))]
mod sys {
    pub(super) fn suspend(_hibernate: bool) -> Result<(), String> {
        Err("sleep and hibernate are not supported on this system".into())
    }

    pub(super) fn shutdown() -> Result<(), String> {
        Err("shutting down is not supported on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Records what was asked; never touches the machine.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
    }

    impl Power for Fake {
        fn suspend(&self, hibernate: bool) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("suspend:{hibernate}"));
            Ok(())
        }
        fn shutdown(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("shutdown".into());
            Ok(())
        }
        fn run(&self, spec: &RunSpec) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("run:{}", spec.program.display()));
            Ok(())
        }
        fn exit(&self) {
            self.calls.borrow_mut().push("exit".into());
        }
    }

    fn program() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("done tool.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let text = exe.to_string_lossy().into_owned();
        (dir, text)
    }

    #[test]
    fn every_action_reaches_the_matching_power_call() {
        let (_dir, exe) = program();
        let cases = [
            (PostAction::Exit, "exit".to_string()),
            (PostAction::Sleep, "suspend:false".into()),
            (PostAction::Hibernate, "suspend:true".into()),
            (PostAction::Shutdown, "shutdown".into()),
            (
                PostAction::RunCommand {
                    command: exe.clone(),
                },
                format!("run:{exe}"),
            ),
        ];
        for (action, expected) in cases {
            let fake = Fake::default();
            perform(&fake, &action).unwrap();
            assert_eq!(*fake.calls.borrow(), vec![expected]);
        }
    }

    #[test]
    fn run_spec_is_the_saved_path_with_no_arguments() {
        let (_dir, exe) = program();
        let spec = build_run_spec(&format!("  \"{exe}\"  ")).unwrap();
        assert_eq!(
            spec.program,
            PathBuf::from(&exe),
            "quotes and spaces dropped"
        );
        assert!(spec.args.is_empty(), "argv is fixed and empty");
        // a path with spaces stays one program, never split into program + arguments
        assert!(exe.contains(' '));
        assert_eq!(build_run_spec(&exe).unwrap(), spec);
    }

    #[test]
    fn run_spec_refuses_anything_that_is_not_a_plain_existing_program() {
        let (dir, exe) = program();
        let missing = dir.path().join("nope.exe").to_string_lossy().into_owned();
        let script = dir.path().join("x.BAT");
        std::fs::write(&script, b"@echo off").unwrap();
        let cases = [
            ("", "empty"),
            ("   ", "blank"),
            ("tool.exe", "relative"),
            (&missing, "missing"),
            (script.to_str().unwrap(), "batch script"),
            (dir.path().to_str().unwrap(), "directory"),
        ];
        for (text, why) in cases {
            assert!(build_run_spec(text).is_err(), "{why} must be refused");
        }
        // Windows ignores trailing dots and spaces, so these still start the script
        let script = script.to_str().unwrap();
        for tricky in [
            format!("{script}."),
            format!("{script} ."),
            format!("{script}. ."),
        ] {
            let err = build_run_spec(&tricky).unwrap_err();
            assert!(
                err.contains("scripts"),
                "{tricky:?} must be refused as a script: {err}"
            );
        }
        let with_control = format!("{exe}\u{7}");
        assert!(build_run_spec(&with_control).is_err(), "control characters");
        let with_args = format!("{exe} /silent");
        assert!(
            build_run_spec(&with_args).is_err(),
            "arguments are not part of the program path"
        );
    }

    #[test]
    fn a_bad_command_never_reaches_the_power_layer() {
        let fake = Fake::default();
        let action = PostAction::RunCommand {
            command: "C:\\definitely\\not\\here.exe".into(),
        };
        assert!(perform(&fake, &action).is_err());
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn validation_only_looks_at_run_command() {
        assert!(validate(&PostAction::Shutdown).is_ok());
        assert!(validate(&PostAction::RunCommand {
            command: "nope".into()
        })
        .is_err());
    }

    #[test]
    fn shutdown_argv_is_fixed_and_only_exit_stops_the_engine() {
        assert_eq!(SHUTDOWN_ARGS, ["/s", "/t", "0"]);
        assert!(stops_engine(&PostAction::Exit));
        assert!(!stops_engine(&PostAction::Shutdown));
        assert!(!stops_engine(&PostAction::Sleep));
    }
}
