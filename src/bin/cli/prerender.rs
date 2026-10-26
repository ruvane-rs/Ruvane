use super::{Result, process, process::Binary, project, watch};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub(super) fn prepare(
    project: &project::Project,
    binary: &mut Binary,
    offline: bool,
) -> Result<Vec<u8>> {
    let artifact = binary.directory.join("prerender.bin");
    binary.ready_files.push(artifact.clone());
    run(&binary.executable, "--ruvane-prerender", &artifact)?;
    let bytes = project::read(&artifact, ruvane::MAX_CLIENT_PRERENDER_BYTES)?;
    let plan = ruvane::ClientPrerender::decode(&bytes)?;
    if plan.revision() != binary.revision {
        return Err("pre-render revision mismatch".into());
    }
    if !plan.is_empty() {
        let target = project.work.join("target");
        let mut command = process::cargo(project, &target, offline);
        command
            .args(["--bin", "ruvane-app", "--release", "--locked"])
            .env(
                "RUVANE_WASM_FILE",
                binary.wasm.as_ref().ok_or("missing application Wasm")?,
            )
            .env("RUVANE_APPLICATION_REVISION", binary.revision.to_string())
            .env("RUVANE_PRERENDER_FILE", &artifact);
        process::run_compiler(command, &AtomicBool::new(false))?;
        let compiled = target.join("release").join(process::EXECUTABLE);
        let executable = binary.directory.join(if cfg!(windows) {
            "ruvane-prerender-app.exe"
        } else {
            "ruvane-prerender-app"
        });
        project::write_new(&executable, &project::read(&compiled, 64 * 1024 * 1024)?)?;
        std::fs::set_permissions(&executable, std::fs::metadata(&compiled)?.permissions())?;
        binary.ready_files.push(binary.executable.clone());
        binary.executable = executable;
        let checked = binary.directory.join("prerender-checked.bin");
        binary.ready_files.push(checked.clone());
        run(&binary.executable, "--ruvane-prerender-verify", &checked)?;
        if project::read(&checked, ruvane::MAX_CLIENT_PRERENDER_BYTES)? != bytes {
            return Err("embedded pre-render artifact mismatch".into());
        }
    }
    if let Some(before) = &binary.sources {
        if before != &watch::snapshot(&project.root)? {
            return Err("sources changed during pre-rendering".into());
        }
    }
    Ok(bytes)
}
fn run(executable: &Path, mode: &str, output: &Path) -> Result<()> {
    let mut child = process::ChildGuard(
        Command::new(executable)
            .arg(mode)
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    process::wait(&mut child, &AtomicBool::new(false), Duration::from_secs(30))
}
