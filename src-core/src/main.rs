#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod discord;
mod elevated_sidecar;
mod flavour;
mod globals;
mod grpc;
mod hardware;
mod http;
mod image_cache;
mod lighthouse;
mod migrations;
mod os;
mod osc;
mod overlay_sidecar;
mod steam;
mod system_tray;
mod utils;
mod vr;
mod vrc_log_parser;
mod vrcx;

use std::process::Command;

use config::Config;
pub use flavour::BUILD_FLAVOUR;
pub use grpc::models as Models;

use globals::{FLAGS, TAURI_APP_HANDLE};
use log::{LevelFilter, error, info, warn};

use oyasumi_shared::{get_log_level, get_log_path};
use tauri::{Manager, Wry, plugin::TauriPlugin};
use tauri_plugin_cli::CliExt;
use tauri_plugin_log::RotationStrategy;

#[macro_export]
macro_rules! warn_unimplemented {
    () => {
        log::error!("unimplemented: {}:{}:{}",file!(),line!(),column!())
    };
    ($($arg:tt)+) => {
        log::error!("unimplemented: {}:{}:{}\n{:?}",file!(),line!(),column!(),format_args!($($arg)+))
    };
}
#[tokio::main]
async fn main() {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    //workaround for webkit bug https://github.com/tauri-apps/tauri/issues/9394
    unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };

    if let Ok(tz) = tz::TimeZone::local()
        && let Ok(tz_name) = tz.find_current_local_time_type()
    {
        unsafe { std::env::set_var("TZ", tz_name.time_zone_designation()) };
    } else {
        eprintln!(
            "failed to set TZ enviroment variable, this may cause execsive cpu usage by webkit"
        );
    }
    //tell oom killer that oyasumi can be killed as one of the first
    std::fs::write("/proc/self/oom_score_adj", "1000").ok();
    let log_path = Box::new(get_log_path());
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.payload_as_str().unwrap_or_default();
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_default();
        let panic_log_path = log_path.join("panic.log");
        let base_log_path = log_path.join("OyasumiVR.log");
        // Write msg and location to file
        eprintln!("Writing panic log to {:#?}", panic_log_path);
        eprintln!(
            "\n{}",
            "#".repeat(term_size::dimensions().unwrap_or_default().0)
        );
        eprintln!(
            "please open an issue https://github.com/sofoxe1/OyasumiVR/issues and include: {:#?} and {:#?}",
            panic_log_path, base_log_path
        );
        eprintln!(
            "{}\n",
            "#".repeat(term_size::dimensions().unwrap_or_default().0)
        );
        let _ = std::fs::write(&*panic_log_path, format!("{} ({})\n", msg, location));
        error!("PANIC: {} ({})", msg, location);
        hook(info);
    }));
    // Attach to parent console if we're running from a command line
    // Construct OyasumiVR Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(configure_tauri_plugin_single_instance())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(configure_tauri_plugin_log())
        .plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let matches = match app.cli().matches() {
                Ok(matches) => Some(matches),
                Err(e) => {
                    eprintln!("Error parsing command line arguments: {e}");
                    app.handle().exit(1);
                    None
                }
            };
            futures::executor::block_on(async {
                *globals::TAURI_CLI_MATCHES.lock().await = matches;
            });
            match futures::executor::block_on(tauri::async_runtime::spawn(app_setup(
                app.handle().clone(),
            ))) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error during Oyasumi's application setup: {e}");
                    app.handle().exit(1);
                }
            }
            Ok(())
        })
        .invoke_handler(configure_command_handlers())
        .on_window_event(system_tray::handle_window_events)
        .run(tauri::generate_context!())
        .expect("An error occurred while running the application")
}

fn configure_tauri_plugin_single_instance() -> TauriPlugin<Wry> {
    tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        // Focus main window when user attempts to launch a second instance.
        let window = app.get_webview_window("main").unwrap();
        if let Ok(is_visible) = window.is_visible() {
            // Window is minimized to tray, show it
            if !is_visible {
                window.show().unwrap();
            }
            // Focus the window
            window.set_focus().unwrap();
        }
    })
}

fn configure_tauri_plugin_log() -> TauriPlugin<Wry> {
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .format(move |out, message, record| {
            let format = time::format_description::parse(
                "[[[year]-[month]-[day]][[[hour]:[minute]:[second]]",
            )
            .unwrap();
            out.finish(format_args!(
                "{}[{}][{}] {}",
                time::OffsetDateTime::now_local()
                    .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
                    .format(&format)
                    .unwrap(),
                record.level(),
                record.module_path().unwrap_or_default(),
                message
            ))
        })
        .rotation_strategy(RotationStrategy::KeepSome(100));
    builder = builder
        .level(get_log_level())
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Stdout,
        ))
        // .level_for("vrchat_osc", LevelFilter::Error)
        // .level_for("xr_overlay", LevelFilter::Debug)
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::LogDir { file_name: None },
        ));

    #[cfg(debug_assertions)]
    {
        builder = builder
            .target(tauri_plugin_log::Target::new(
                tauri_plugin_log::TargetKind::Webview,
            ))
            .level_for("xr_overlay", LevelFilter::Trace)
            .level(LevelFilter::Debug)
        // .level_for("vrchat_osc", LevelFilter::Warn);
    }

    builder.build()
}

async fn app_setup(app_handle: tauri::AppHandle) {
    fn get_gpus() -> Box<[String]> {
        let mut vec = Vec::new();
        match Command::new("glxinfo").output() {
            Ok(v) => {
                let s = String::from_utf8_lossy(&v.stdout).to_string();
                for l in s.lines() {
                    if l.starts_with("OpenGL renderer string: ") {
                        vec.push(
                            l.chars()
                                .skip("OpenGL renderer string: ".len())
                                .collect::<String>(),
                        );
                    }
                }
            }
            Err(_) => {
                if let Ok(v) = Command::new("lspci").arg("-nn").output() {
                    let s = String::from_utf8_lossy(&v.stdout).to_string();
                    for l in s.lines() {
                        if l.contains("VGA compatible controller") {
                            let l = l.split(":").nth(2);
                            if l.is_none() {
                                continue;
                            }
                            let l = l.unwrap();
                            let mut l = l
                                .chars()
                                .skip_while(|x| *x != '[')
                                .collect::<String>()
                                .split(" ")
                                .map(|s| s.to_string())
                                .collect::<Vec<String>>();
                            l.pop();
                            let l = l.into_iter().map(|s| format!("{} ", s)).collect::<String>();
                            vec.push(l);
                        }
                    }
                }
            }
        };
        if vec.is_empty() {
            vec.push("Unknown".to_string());
        }
        vec.into_boxed_slice()
    }
    let release = os_release::OS_RELEASE
        .as_ref()
        .map(|r| format!("{} {}", r.name, r.version_id))
        .unwrap_or("Unknown".into());
    info!(
        "[Core] Specs:\n Distro: {}\n Gpus:\n {:#?}",
        release,
        get_gpus()
    );
    info!(
        "[Core] Starting OyasumiVR in {} mode",
        crate::utils::cli_core_mode().await
    );
    // Ensure the working directory is the installation directory
    let executable_path = {
        let full_path = std::env::current_exe().unwrap();
        full_path.parent().unwrap().to_path_buf()
    };
    info!("[Core] Setting working directory to: {:?}", executable_path);
    std::env::set_current_dir(&executable_path).unwrap();

    // Load configs
    load_configs().await;
    // Set up app reference
    *TAURI_APP_HANDLE.lock().await = Some(app_handle.clone());

    // Open devtools if we're in debug mode
    #[cfg(debug_assertions)]
    {
        let window = app_handle.get_webview_window("main").unwrap();
        window.open_devtools();
    }

    // Get dependencies
    let cache_dir = app_handle.path().app_cache_dir().unwrap();
    // Register deep link schemas if needed
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        if let Err(e) = app_handle.deep_link().register_all() {
            error!("[Core] Failed to register deep link schemas: {}", e);
        }
    }
    utils::init().await;
    // Initialize Steam module
    #[cfg(feature = "steam")]
    steam::init().await;
    // Initialize HTTP server
    http::init().await;
    // Initialize gRPC server
    grpc::init_server().await;
    grpc::init_web_server().await;

    // Initialize VR Manager
    vr::init().await;
    // Initialize Image Cache
    image_cache::init(cache_dir).await;
    // Init sound playback
    os::init_sound_playback().await;

    // Initialize Lighthouse Bluetooth
    lighthouse::init().await;
    // Initialize log commands
    commands::log_utils::init(app_handle.path().app_log_dir().unwrap()).await;
    // Initialize overlay sidecar module
    overlay_sidecar::init().await;
    // Initialize Discord module
    discord::init().await;
    // Initialize system tray
    system_tray::init().await;

    // Start profiling if we're in debug mode
    // #[cfg(debug_assertions)]
    // {
    //     utils::profiling::enable_profiling();
    // }
    // Start profiling if the flag for it is set
    #[cfg(all(not(debug_assertions), feature = "profiling"))]
    if globals::is_flag_set("ENABLE_PROFILING").await {
        utils::profiling::enable_profiling();
    }
}

async fn load_configs() {
    match Config::builder()
        .add_source(config::File::with_name("flags"))
        .build()
    {
        Ok(flags) => {
            *FLAGS.lock().await = Some(flags);
        }
        Err(e) => match e {
            config::ConfigError::NotFound(_) => {
                warn!("[Core] Could not find flags config. Using default values.");
            }
            _ => {
                warn!("[Core] Could not load flags config: {:#?}", e);
            }
        },
    };
}

fn configure_command_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        vr::commands::vr_get_devices,
        vr::commands::vr_status,
        vr::commands::vr_get_analog_gain,
        vr::commands::openvr_set_analog_gain,
        vr::commands::vr_set_image_brightness,
        vr::commands::openvr_reregister_manifest,
        vr::commands::openvr_set_init_delay_fix,
        vr::commands::vr_set_analog_color_temp,
        vr::commands::vr_set_app_framelimit,
        // vr::commands::vr_get_app_framelimit,
        vr::commands::vr_sleep_mode_check,
        vr::commands::vr_sleep_detection_enabled,
        vr::commands::set_sleep_state,
        os::commands::is_windows,
        os::commands::run_command,
        os::commands::run_cmd_commands,
        os::commands::play_sound,
        os::commands::show_in_folder,
        os::commands::quit_steamvr,
        os::commands::get_system_power_policies,
        os::commands::set_system_power_policy,
        os::commands::active_system_power_policy,
        os::commands::system_shutdown,
        os::commands::system_reboot,
        os::commands::system_sleep,
        os::commands::system_logout,
        os::commands::system_hibernate,
        os::commands::windows_is_elevated,
        os::commands::get_audio_devices,
        os::commands::set_audio_device_volume,
        os::commands::set_audio_device_mute,
        os::commands::set_mic_activity_device_id,
        os::commands::set_hardware_mic_activity_enabled,
        os::commands::set_hardware_mic_activivation_threshold,
        os::commands::is_vrchat_active,
        os::commands::is_elevation_security_disabled,
        os::commands::pause_mpris_players,
        osc::commands::osc_send_command,
        osc::commands::osc_valid_addr,
        osc::commands::start_osc_server,
        osc::commands::stop_osc_server,
        osc::commands::get_vrchat_osc_address,
        osc::commands::get_vrchat_oscquery_address,
        osc::commands::add_osc_method,
        osc::commands::set_osc_method_value,
        osc::commands::set_osc_receive_address_whitelist,
        elevated_sidecar::commands::elevated_sidecar_started,
        elevated_sidecar::commands::start_elevated_sidecar,
        elevated_sidecar::commands::elevated_sidecar_get_grpc_web_port,
        elevated_sidecar::commands::elevated_sidecar_get_grpc_port,
        overlay_sidecar::commands::start_overlay_sidecar,
        overlay_sidecar::commands::stop_overlay_sidecar,
        overlay_sidecar::commands::overlay_sidecar_get_grpc_web_port,
        overlay_sidecar::commands::overlay_sidecar_get_grpc_port,
        system_tray::commands::set_close_to_system_tray,
        vrc_log_parser::commands::init_vrc_log_watcher,
        discord::commands::discord_update_activity,
        discord::commands::discord_clear_activity,
        http::commands::get_http_server_port,
        image_cache::commands::clean_image_cache,
        lighthouse::commands::lighthouse_start_scan,
        lighthouse::commands::lighthouse_get_devices,
        lighthouse::commands::lighthouse_set_device_power_state,
        lighthouse::commands::lighthouse_get_device_power_state,
        lighthouse::commands::lighthouse_get_status,
        lighthouse::commands::lighthouse_get_scanning_status,
        lighthouse::commands::lighthouse_reset,
        steam::commands::steam_active,
        steam::commands::steam_achievement_get,
        steam::commands::steam_achievement_set,
        commands::log_utils::clear_log_files,
        commands::afterburner::gpu_set_profile,
        commands::afterburner::gpu_get_profiles,
        commands::notifications::xsoverlay_send_message,
        commands::splash::close_splashscreen,
        commands::nvml::nvml_status,
        commands::nvml::nvml_get_devices,
        commands::nvml::nvml_set_power_management_limit,
        commands::debug::open_dev_tools,
        commands::debug::is_flag_set,
        commands::time::get_sunrise_sunset_time,
        grpc::commands::get_core_grpc_port,
        grpc::commands::get_core_grpc_web_port,
        vrcx::commands::vrcx_log,
        os::commands::n_os_inhibit,
        os::commands::n_os_un_inhibit,
    ]
}
