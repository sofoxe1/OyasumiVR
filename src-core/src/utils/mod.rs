use log::{error, info, trace};
use serde::Serialize;
use std::ffi::OsStr;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal};
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::globals::{TAURI_APP_HANDLE, TAURI_CLI_MATCHES};
use crate::osc::commands::OSC_SERVER;
static SYSINFO: LazyLock<Mutex<sysinfo::System>> =
    LazyLock::new(|| Mutex::new(sysinfo::System::new()));

pub mod models;
#[cfg(feature = "profiling")]
pub mod profiling;
pub mod serialization;
pub mod sidecar_manager;
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackedProcess {
    Steamvr,
    Vrchat,
    MonadoService,
    Wivrn,
}
impl TrackedProcess {
    pub fn name(&self) -> &OsStr {
        OsStr::new(match self {
            Self::Steamvr => "vrmonitor",
            Self::Vrchat => "VRChat.exe",
            Self::MonadoService => "monado-service",
            Self::Wivrn => "wivrn-server",
        })
    }
}
pub async fn init() {
    tokio::task::spawn(watch_vrchat_process_osc());
}
pub static mut VRCHAT_ACTIVE: bool = false;
//in case vrchat crashes
async fn watch_vrchat_process_osc() {
    loop {
        //this is only meant to eventually set not running in case of a crash\
        //large wait value so it doesnt actually require osc to be enabled
        //oyasumi would still work fine if the frontend thought vrchat was always active anyways
        tokio::time::sleep(Duration::from_mins(2)).await;
        if unsafe {
            SystemTime::now()
                .duration_since(LAST_ACTIVE)
                .unwrap_or_default()
                < Duration::from_mins(2)
        } {
            unsafe {
                log::debug!(
                    "[vrc_heartbeat] vrchat seen recently:{:?} ago",
                    SystemTime::now()
                        .duration_since(LAST_ACTIVE)
                        .unwrap_or_default()
                );
            }
            continue;
        }
        unsafe {
            log::debug!(
                "[vrc_heartbeat] vrchat not seen for:{:?}",
                SystemTime::now()
                    .duration_since(LAST_ACTIVE)
                    .unwrap_or_default()
            );
        }

        {
            let mut guard = OSC_SERVER.lock().await;
            if let Some(server) = guard.as_mut() {
                let active = server
                    .get_parameter("/avatar/parameters/MuteSelf", "VRChat-Client-*")
                    .await
                    .map(|res| !res.is_empty())
                    .unwrap_or(false);
                log::debug!("[vrc_heartbeat] fetching mute state:{}", active);
                if active {
                    trace!("[vrc_heartbeat] osc set active");
                    set_vrchat_active().await;
                } else {
                    trace!("[vrc_heartbeat] osc set inactive");
                    set_vrchat_inactve().await;
                }
            }
        }
    }
}
static mut LAST_ACTIVE: SystemTime = SystemTime::UNIX_EPOCH;
pub async fn set_vrchat_active() {
    unsafe {
        LAST_ACTIVE = SystemTime::now();
        if !VRCHAT_ACTIVE {
            info!("[core] Detected VRChat process has started");
            crate::utils::send_event("VRCHAT_PROCESS_ACTIVE", true).await;
            VRCHAT_ACTIVE = true;
        }
    }
}
pub async fn set_vrchat_inactve() {
    unsafe {
        if VRCHAT_ACTIVE {
            info!("[core] Detected VRChat process has stopped");
            crate::utils::send_event("VRCHAT_PROCESS_ACTIVE", false).await;
            VRCHAT_ACTIVE = false;
        }
    }
}

pub async fn is_process_active(process: TrackedProcess) -> bool {
    static ACTIVE_PROCESS: Mutex<Vec<(TrackedProcess, Pid)>> = Mutex::const_new(Vec::new());
    let mut sysinfo_guard = SYSINFO.lock().await;
    let mut active_guard = ACTIVE_PROCESS.lock().await;
    //first check if process which previously matches the name still exists
    if let Some(p) = active_guard.iter().position(|p| p.0 == process) {
        sysinfo_guard.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[active_guard[p].1]),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        if sysinfo_guard
            .processes_by_exact_name(active_guard[p].0.name())
            .next()
            .is_some()
        {
            true
        } else {
            active_guard.swap_remove(p);
            false
        }
    } else {
        sysinfo_guard.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let processes = sysinfo_guard
            .processes_by_exact_name(process.name())
            .collect::<Vec<_>>();
        if processes.is_empty() {
            false
        } else {
            //even if there is multiple processes matching the name as long as one of them is running sysinfo would return something
            active_guard.push((process, processes[0].pid()));
            true
        }
    }
}
pub async fn quit_steamvr(kill: bool) {
    let sysinfo_guard = SYSINFO.lock().await;
    for p in [
        TrackedProcess::Steamvr,
        TrackedProcess::Wivrn,
        TrackedProcess::MonadoService,
    ] {
        if is_process_active(p).await {
            //is_process_active already refreshes processes
            for process in sysinfo_guard.processes_by_exact_name(TrackedProcess::Steamvr.name()) {
                if kill
                    || (process.kill_with(Signal::Term).is_none()
                        && process.kill_with(Signal::Quit).is_none())
                {
                    let _ = process.kill_with(Signal::Kill);
                }
            }
        }
    }
}

pub fn get_time() -> u64 {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    since_the_epoch.as_millis() as u64
}

pub async fn send_event<S: Serialize + Clone>(event: &str, payload: S) {
    #[cfg(feature = "profiling")]
    profiling::register_event(event).await;
    let app_handle_guard = TAURI_APP_HANDLE.lock().await;
    let app_handle = app_handle_guard.as_ref().unwrap();
    match app_handle.emit(event, payload) {
        Ok(_) => {}
        Err(e) => {
            error!("[Core] Failed to send event {}: {}", event, e);
        }
    };
}

pub async fn cli_core_mode() -> models::CoreMode {
    let default = "release";
    let match_guard = TAURI_CLI_MATCHES.lock().await;
    let mode = match match_guard.as_ref().unwrap().args.get("core-mode") {
        Some(data) => data.value.as_str().unwrap_or(default),
        None => default,
    };
    // Determine the correct mode
    match mode {
        "dev" => models::CoreMode::Dev,
        "release" => models::CoreMode::Release,
        _ => {
            error!("[Core] Invalid core mode specified. Defaulting to release mode.");
            models::CoreMode::Release
        }
    }
}

pub async fn cli_sidecar_overlay_mode() -> models::OverlaySidecarMode {
    let default = "release";
    let match_guard = TAURI_CLI_MATCHES.lock().await;
    let mode = match match_guard
        .as_ref()
        .unwrap()
        .args
        .get("overlay-sidecar-mode")
    {
        Some(data) => data.value.as_str().unwrap_or(default),
        None => default,
    };
    // Determine the correct mode

    match mode {
        "dev" => models::OverlaySidecarMode::Dev,
        "release" => models::OverlaySidecarMode::Release,
        _ => {
            error!("[Core] Invalid overlay sidecar mode specified. Defaulting to release mode.");
            models::OverlaySidecarMode::Release
        }
    }
}
