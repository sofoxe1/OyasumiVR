use std::{
    fs::{self, File},
    net::TcpStream,
    path::PathBuf,
    sync::{LazyLock, Mutex, OnceLock},
    time::Duration,
};

use log::{info, trace};
use oyasumi_shared::{OVERLAY_CONFIG_PATH, XR_BINDING_FILE_PATH, get_log_level, get_log_path};
use tonic::transport::Channel;
use xr_overlay::{
    openxr::{Posef, Quaternionf, Vector3f},
    runner::DeviceRole,
};
use xr_overlay_cef::{
    cef::{ImplBrowser, ImplFrame},
    disable_gpu, disable_vr, pointless_cef_thread_spawner,
};

use crate::{
    config::{DEFAULT_OVERLAY_CONFIG, OverlayConfig},
    core_grpc::{Empty, OverlaySidecarStartArgs, oyasumi_core_client::OyasumiCoreClient},
    globals::CORE_GRPC_DEV_PORT,
    grpc::{start_grpc_server, start_grpc_web_server},
    logger::{Writter, get_o_log_path},
    overlay_ipc::start_websocket_server,
    ui::serve_ui,
    vr::{
        CACHE_PATH, DEFAULT_BINDINGS_CONFIG, NOTIFICATION_OVERLAY, OVERLAY, SPLASH_PLAYED, XR_CTX,
        hide_dashboard, show_dashboard, start_vr,
    },
};
pub mod config;
pub mod globals;
pub mod grpc;
pub mod input;
mod logger;
pub mod model;
pub mod overlay_ipc;
pub mod ui;
pub mod vr;
pub mod core_grpc {
    tonic::include_proto!("oyasumi_core");
}
pub mod overlay_grpc {
    tonic::include_proto!("oyasumi_overlay_sidecar");
}
#[macro_export]
macro_rules! warn_unimplemented {
    () => {
        log::error!("unimplemented: {}:{}:{}",file!(),line!(),column!())
    };
    ($($arg:tt)+) => {
        log::error!("unimplemented: {}:{}:{}\n{:?}",file!(),line!(),column!(),format_args!($($arg)+))
    };
}
static NO_VR: OnceLock<bool> = OnceLock::new();
static ARGS: OnceLock<Args> = OnceLock::new();
pub static CONFIG: OnceLock<OverlayConfig> = OnceLock::new();
#[derive(Clone, Copy, Debug)]
pub struct Args {
    core_grpc_port: u16,
    core_pid: u64,
    disable_gpu: bool,
}
fn main() {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    let hook = std::panic::take_hook();
    let panic_log_path = get_log_path().join("overlay_panic.log");
    std::panic::set_hook(Box::new(move |e| {
        let path = format!(
            "overlay_panic_{}_{:?}.log",
            std::process::id(),
            std::thread::current().id()
        );

        std::fs::write(
            panic_log_path.join(PathBuf::from(&path)),
            format!("{:?}", &e),
        )
        .ok();
        println!(
            "Writing panic log to {:#?} open an issue https://github.com/sofoxe1/OyasumiVR/issues and inclue {:#?} ",
            panic_log_path,
            oyasumi_shared::get_log_path().join("overlay.log")
        );
        log::error!("PANIC: {:?}", e);
        hook(e);
    }));

    let mut binding = env_logger::Builder::new();

    let logger = binding.filter_level(get_log_level());
    static mut MAIN: bool = false;
    // #[cfg(debug_assertions)]
    // {
    //     logger = logger
    //         .filter_module("xr_overlay_cef", log::LevelFilter::Debug)
    //         .filter_module("xr_overlay", log::LevelFilter::Debug)
    //         .filter_module("tokio_tungstenite", log::LevelFilter::Warn)
    //         .filter_module("tungstenite", log::LevelFilter::Warn);
    // }

    let w = Writter::default();
    let f_ = w.file.clone();

    logger
        .target(env_logger::Target::Pipe(Box::new(w)))
        .parse_default_env()
        .init();

    if !OVERLAY_CONFIG_PATH.exists() {
        fs::write(&*OVERLAY_CONFIG_PATH, DEFAULT_OVERLAY_CONFIG).unwrap();
    }

    trace!("args: {:?}", std::env::args());
    trace!(
        "thread_id:{:?},pid:{:?}",
        std::thread::current().id(),
        std::process::id()
    );

    fs::write("/proc/self/oom_score_adj", "1000").ok();
    pointless_cef_thread_spawner();
    let f = File::create(get_o_log_path()).unwrap();
    f_.lock().unwrap().replace(f);

    unsafe { MAIN = true };

    trace!(
        "main thread_id:{:?},pid:{:?}",
        std::thread::current().id(),
        std::process::id()
    );
    log::trace!("args:{:?}", std::env::args());
    let args = std::env::args().collect::<Vec<_>>();
    if !(args.len() == 4 || args.len() == 3) {
        panic!("Usage: oyasumivr-overlay-sidecar <core grpc port> <core process id>")
    }

    let mut args = Args {
        core_grpc_port: args[1].parse().unwrap(),
        core_pid: args[2].parse().unwrap(),
        disable_gpu: args.get(3).cloned().unwrap_or_default() == "--disable-gpu-acceleration",
    };
    if args.disable_gpu {
        disable_gpu();
    }
    if args.core_grpc_port == 0 && args.core_pid == 0 {
        args.core_grpc_port = globals::CORE_GRPC_DEV_PORT;
    }
    log::debug!("using arguments:{:?}", args);
    ARGS.set(args).unwrap();
    NO_VR
        .set(std::env::var("NO_VR").unwrap_or_default().to_lowercase() == "true")
        .unwrap();
    log::debug!("NO_VR:{:?}", NO_VR.get().as_ref().unwrap());
    if *NO_VR.get().unwrap() {
        disable_vr();
    }
    if ARGS.get().as_ref().unwrap().core_pid != 0 && !XR_BINDING_FILE_PATH.is_file() {
        fs::write(&*XR_BINDING_FILE_PATH, DEFAULT_BINDINGS_CONFIG).unwrap();
    }
    let vr_thread = match start_vr() {
        Some(v) => v,
        None => {
            log::trace!("early kill");
            return;
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(tokio_main());

    vr_thread.join().unwrap();
    trace!("exiting");
    unsafe { xr_overlay_cef::shutdown() };
    trace!("cef shutdown");
    runtime.shutdown_timeout(Duration::from_millis(100));
    trace!("runtime exited");
    //clean up cef cache
    fs::remove_dir(CACHE_PATH.clone()).ok();
}
static HANDLES: LazyLock<Mutex<Vec<tokio::task::JoinHandle<()>>>> = LazyLock::new(Mutex::default);

static CORE_CLIENT: OnceLock<tokio::sync::Mutex<OyasumiCoreClient<Channel>>> = OnceLock::new();
static UI_PORT: OnceLock<u16> = OnceLock::new();
static HTTP_PORT: OnceLock<u16> = OnceLock::new();
static WS_PORT: OnceLock<u16> = OnceLock::new();
async fn tokio_main() {
    trace!("tokio_main");
    tokio::task::spawn(async {
        let pid = ARGS.get().as_ref().unwrap().core_pid as u32;
        if pid == 0 {
            trace!("core_pid 0");
            return;
        }
        log::trace!("watching:{} pid", pid);
        loop {
            if killed() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
    let mut core_client = OyasumiCoreClient::connect(format!(
        "http://127.0.0.1:{}",
        ARGS.get().as_ref().unwrap().core_grpc_port
    ))
    .await
    .unwrap();
    CORE_CLIENT.set(core_client.clone().into()).unwrap();
    log::info!("connected to core");
    let http_port = core_client
        .get_http_server_port(Empty {})
        .await
        .unwrap()
        .into_inner()
        .port;
    info!("got http port:{:?}", http_port);
    let ui_port = match ARGS.get().as_ref().unwrap().core_grpc_port == CORE_GRPC_DEV_PORT {
        true => {
            if cfg!(debug_assertions) && TcpStream::connect("localhost:5173").is_ok() {
                log::debug!("using: 127.0.0.1:5173 for ui");
                5173
            } else {
                log::debug!("serving ui");
                serve_ui().await
            }
        }
        false => serve_ui().await,
    };
    HTTP_PORT.set(http_port as u16).unwrap();
    UI_PORT.set(ui_port).unwrap();

    log::info!("ui port:{}", ui_port);
    std::thread::sleep(Duration::from_millis(100));
    let url = format!("http://localhost:{}/splash?corePort={}", ui_port, http_port);
    unsafe { SPLASH_PLAYED = true };
    let url_noti = format!(
        "http://localhost:{}/notifications?corePort={}",
        ui_port, http_port
    );
    // let url_noti="https://google.com".to_string();
    trace!("navigating to:{}", url);
    let ws_port = start_websocket_server().await;
    WS_PORT.set(ws_port).unwrap();
    log::info!("ws port:{}", ws_port);
    OVERLAY
        .get()
        .as_ref()
        .unwrap()
        .browser
        .main_frame()
        .unwrap()
        .load_url(Some(&(url.as_str()).into()));
    std::thread::sleep(Duration::from_millis(20)); //this is how you fix race conditions :3
    OVERLAY.wait().inject_ipc(ws_port);

    let grpc_server_port = start_grpc_server().await;
    let grpc_web_server_pos = start_grpc_web_server().await;
    log::info!(
        "grpc:{}, grpc_web:{}",
        grpc_server_port,
        grpc_web_server_pos
    );
    core_client
        .on_overlay_sidecar_start(OverlaySidecarStartArgs {
            pid: std::process::id(),
            grpc_port: grpc_server_port as u32,
            grpc_web_port: grpc_web_server_pos as u32,
        })
        .await
        .unwrap();
    log::info!("sent onstart");
    assert_ne!(grpc_web_server_pos, 0);
    assert_ne!(grpc_server_port, 0);
    XR_CTX
        .get()
        .unwrap()
        .write()
        .unwrap()
        .set_posef_relative(
            DeviceRole::Hmd,
            OVERLAY.get().unwrap().xr_handle,
            Posef {
                orientation: Quaternionf::IDENTITY,
                position: Vector3f {
                    x: 0.0,
                    y: -0.2,
                    z: -1.2,
                },
            },
            true,
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    log::info!("initial overlay show");
    show_dashboard();
    tokio::task::spawn(async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        hide_dashboard().await;
    });
    NOTIFICATION_OVERLAY
        .get()
        .as_ref()
        .unwrap()
        .browser
        .main_frame()
        .unwrap()
        .load_url(Some(&(url_noti.as_str()).into()));
    NOTIFICATION_OVERLAY.wait().inject_ipc(ws_port);
    log::info!("overlays ready");
    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(no_vr) = NO_VR.get()
        && *no_vr
    {
        show_dashboard();
    }
}
static mut KILL: bool = false;
// #[allow(dead_code)]
#[inline]
pub fn kill() {
    if !killed() {
        trace!("killing overlay");
    }
    unsafe { KILL = true };
}
#[inline]
pub fn killed() -> bool {
    let pid = ARGS.get().as_ref().unwrap().core_pid as u32;
    if pid != 0 && !PathBuf::from(format!("/proc/{}", pid)).exists() {
        unsafe { KILL = true };
    }

    unsafe { KILL }
}
