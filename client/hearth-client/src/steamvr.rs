//! Starting on a different OpenXR runtime makes a running SteamVR drop its
//! session and quit, so point the loader at SteamVR when SteamVR is up. This
//! has to happen before the engine creates its OpenXR instance, which is why
//! it runs at extension init.

#[cfg(not(target_os = "windows"))]
pub fn prefer_if_running() {}

#[cfg(target_os = "windows")]
pub fn prefer_if_running() {
    if std::env::var_os("XR_RUNTIME_JSON").is_some() {
        return; // The player already picked a runtime.
    }
    if !is_running("vrmonitor.exe") {
        return;
    }
    let Some(path) = runtime_json() else {
        return;
    };
    // Safety: extension init, before the engine starts any other thread.
    unsafe { std::env::set_var("XR_RUNTIME_JSON", &path) };
    godot::global::godot_print!("SteamVR is running, using {path}");
}

#[cfg(target_os = "windows")]
fn is_running(exe: &str) -> bool {
    run("tasklist", &["/FI", &format!("IMAGENAME eq {exe}"), "/NH"])
        .is_some_and(|out| out.to_lowercase().contains(exe))
}

#[cfg(target_os = "windows")]
fn runtime_json() -> Option<String> {
    let from_registry = steam_path().map(|dir| format!(r"{dir}\steamapps\common\SteamVR\steamxr_win64.json"));
    let default = r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json".to_owned();
    [from_registry, Some(default)]
        .into_iter()
        .flatten()
        .find(|path| std::path::Path::new(path).exists())
}

#[cfg(target_os = "windows")]
fn steam_path() -> Option<String> {
    let out = run("reg", &["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])?;
    let line = out.lines().find(|line| line.contains("SteamPath"))?;
    let (_, value) = line.split_once("REG_SZ")?;
    Some(value.trim().replace('/', "\\"))
}

#[cfg(target_os = "windows")]
fn run(program: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}
