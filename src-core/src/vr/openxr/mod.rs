use std::{
    sync::{LazyLock, OnceLock},
    time::Duration,
};
mod input;
use glam::{Quat, Vec3A};
use log::{debug, info};
use serde_repr::Serialize_repr;
use tokio::{spawn, sync::Mutex};
use xr_overlay::{
    RgbaTexture,
    error::LocateError,
    model::AppContext,
    openxr::{Posef, Vector3f},
    runner::{AppRunner, AppRunnerCreateInfo, OverlayCreateInfo, OverlayHandle, events::AppEvent},
    utils::{QuaternionfExt, VecFExt},
    xr::ReferenceSpaceT,
};

use crate::{
    utils::send_event,
    vr::{
        SLEEP_DETECTION_ENABLED,
        commands::SLEEP_STATE,
        gesture_detector::GestureDetector,
        model::{SleepState, VRStatus},
        openxr::input::{INPUT_CONTEXT, check_user_activity},
        sleep_detector::{SLEEP_DETECTOR_PERIOD, SleepDetector},
    },
};
pub static OXR_HANDLE: OnceLock<Mutex<AppRunner>> = OnceLock::new();
pub static OXR_BRIGHTNES_OVERLAY_HANDLE: Mutex<Option<OverlayHandle>> = Mutex::const_new(None);
pub static OXR_STATE: Mutex<VRStatus> = Mutex::const_new(VRStatus::Inactive);

async fn get_ctx() -> AppContext<xr_overlay::openxr::Vulkan> {
    let ctx = loop {
        let ctx = xr_overlay::xr::Init::default()
            .disable_hand_tracking()
            .sort_order(u16::MAX as u32)
            .user_presence_support(true)
            .with_app_name("Oyasumi VR");
        // if let Some(ref app)=app{
        //     ctx=ctx.with_instance(&unsafe { app.get_ctx() }.xr.instance);
        // }
        let ctx = ctx.init_overlay();
        if let Err(_e) = ctx {
            if !matches!(xr_overlay::error::Error::RuntimeUnavalible, _e) {
                log::warn!("get_ctx {:?}", _e);
            }
            log::info!("get ctx {:?}", _e);
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        } else {
            break ctx.unwrap();
        }
    };
    update_status(VRStatus::Initializing).await;
    ctx
}
fn get_overlay_info() -> OverlayCreateInfo {
    OverlayCreateInfo {
        type_: xr_overlay::runner::OverlayCreateInfoType::Unmanaged {
            size: [1., 1.].into(),
        },
        spawn_visible: true,
        //make sure it's in front of the user
        pos: Vector3f {
            x: 0.0,
            y: 0.0,
            z: -0.1,
        },
        ..Default::default()
    }
}
pub async fn init() {
    tokio::task::spawn(async {
        let ctx = get_ctx().await;

        info!("[Init] connected to openxr");

        let mut runner = xr_overlay::runner::AppRunner::new(AppRunnerCreateInfo {
            ctx: ctx.clone(),
            space_type: ReferenceSpaceT::VIEW,
            callback: openxr_callback,
        });
        let brightness_overlay_handle = runner.add_overlay(get_overlay_info());

        OXR_BRIGHTNES_OVERLAY_HANDLE
            .lock()
            .await
            .replace(brightness_overlay_handle);
        OXR_HANDLE.set(Mutex::new(runner)).unwrap();
        match input::get_input_handlers(ctx.clone()) {
            Some(v) => *INPUT_CONTEXT.lock().await = Some(v),
            None => log::warn!(
                "failed to create input context, button pressing and mic mute will not work"
            ),
        };
        tokio::task::spawn(async {
            loop {
                if *OXR_STATE.lock().await == VRStatus::Initialized {
                    let mut xr_ctx = OXR_HANDLE.get().unwrap().lock().await;

                    match xr_ctx.run() {
                        xr_overlay::runner::PollResult::Success(_) => (),
                        xr_overlay::runner::PollResult::UserNotPresent => {
                            drop(xr_ctx);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                        xr_overlay::runner::PollResult::Exit
                        | xr_overlay::runner::PollResult::SessionLost => {
                            drop(xr_ctx);
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            debug_assert_eq!(*OXR_STATE.lock().await, VRStatus::Inactive);
                            continue;
                        }
                        xr_overlay::runner::PollResult::Starting => {
                            drop(xr_ctx);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                        xr_overlay::runner::PollResult::SuccessNoRender => {
                            drop(xr_ctx);
                            tokio::time::sleep(Duration::from_secs(60)).await
                        }
                    }
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        tokio::task::spawn(async move {
            let mut last_pose = SIDE::Front;
            loop {
                let pose = get_pose("sleep", &mut *OXR_HANDLE.wait().lock().await).await;
                if let Some(pose) = pose {
                    #[allow(clippy::collapsible_if)] //no????
                    if unsafe { SLEEP_DETECTION_ENABLED && SLEEP_STATE != SleepState::Sleeping } {
                        SLEEP_DETECTOR
                            .lock()
                            .await
                            .log_pose(pose.position.to_vec3a().to_vec3())
                            .await;
                    }
                    let c_pose = get_side(pose.orientation.to_quat());
                    if c_pose != last_pose {
                        last_pose = c_pose;
                        send_event("POSE", c_pose).await;
                    }
                }
                tokio::time::sleep(SLEEP_DETECTOR_PERIOD).await;
            }
        });
        debug!("[Init] openxr start (2)");
        update_status(VRStatus::Initialized).await;
    });
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize_repr)]
#[repr(u8)]
pub enum SIDE {
    Back = 0,
    Left = 1,
    Right = 2,
    Front = 3,
}
#[inline]
fn get_side(quat: Quat) -> SIDE {
    if !{ quat.mul_vec3a(Vec3A::Y).dot(Vec3A::Y).abs() < 0.62 } {
        SIDE::Front
    } else {
        let side = quat.mul_vec3a(Vec3A::X).dot(Vec3A::Y);
        if side.abs() > 0.62 {
            match side.is_sign_negative() {
                true => SIDE::Right,
                false => SIDE::Left,
            }
        } else {
            //oyasumi doesn't distunguish between laying face down and up
            SIDE::Back
        }
    }
}
#[inline]
async fn get_pose(src: &'static str, ctx: &mut AppRunner) -> Option<Posef> {
    if !ctx.is_user_present() {
        tokio::time::sleep(Duration::from_secs(1)).await;
        return None;
    }
    let res = ctx.get_hmd_posef(ReferenceSpaceT::STAGE);
    if let Ok(posef) = res {
        return Some(posef);
    } else if let Err(err) = res {
        match err {
            LocateError::RuntimeFailure
            | LocateError::InstanceLost
            | LocateError::SessionLost
            | LocateError::HandleInvalid => {
                let _ = OXR_HANDLE.get().unwrap().lock().await.run();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            _ => (),
        };
        if !ctx.is_runtime_active() {
            let _ = ctx.run();
            tokio::time::sleep(Duration::from_secs(1)).await;
            return None;
        }
        if err == LocateError::LocationEmpty {
            debug!("[Core] Failed to get hmd Posef,{}:{:?}", src, err);
        }
    }
    None
}
async fn session_restart() {
    let mut handle = OXR_HANDLE.get().as_ref().unwrap().lock().await;
    let ctx = get_ctx().await;
    debug!("got ctx");
    match input::get_input_handlers(ctx.clone()) {
        Some(v) => *INPUT_CONTEXT.lock().await = Some(v),
        None => {
            log::warn!("failed to create input context, button pressing and mic mute will not work")
        }
    };
    debug!("got input");
    unsafe { handle.replace_ctx(xr_overlay::runner::SessionRestartInfo::NoInput { ctx }) };
    debug!("ctx replaced");
    let overlay_handle = handle.add_overlay(get_overlay_info());
    OXR_BRIGHTNES_OVERLAY_HANDLE
        .lock()
        .await
        .replace(overlay_handle);
    set_brightness(1.0, None).await;
    let _ = handle.run();
    debug!("run");
    update_status(VRStatus::Initialized).await;
    debug!("session restarted");

    unsafe { RESTARTING = false };
}
//no need for atomic since vr is running on single thread
static mut RESTARTING: bool = false;
fn openxr_callback(event: AppEvent) {
    match event {
        AppEvent::SessionEnded | AppEvent::Killed => {
            if !unsafe { RESTARTING } {
                unsafe { RESTARTING = true };

                log::debug!("[core] openxr disconnected");
                spawn(async {
                    update_status(VRStatus::Inactive).await;
                    INPUT_CONTEXT.lock().await.take();
                    unsafe { OXR_HANDLE.get().as_ref().unwrap().lock().await.drop_ctx() };
                    OXR_BRIGHTNES_OVERLAY_HANDLE.lock().await.take();
                    tokio::task::spawn(session_restart());
                });
            }
        }
        AppEvent::Started => {
            log::debug!("[core] openxr ready");
            spawn(update_status(VRStatus::Initialized));
        }
        _ => (),
    }
}
async fn update_status(new_status: VRStatus) {
    info!("[core] updating openxr status:{:?}", new_status);
    if *OXR_STATE.lock().await == VRStatus::Initialized && new_status == VRStatus::Initializing {
        unreachable!("possible race condition for update_status"); //panic instead of error since this is a logic error and need to be fixed
    }
    *OXR_STATE.lock().await = new_status;
    send_event("VR_STATUS_UPDATE", new_status.to_string().to_uppercase()).await;
}
static ABORT_GESTURE_DETECTION: Mutex<bool> = Mutex::const_new(false);
static GESTURE_DETECTION_RUNNING: Mutex<bool> = Mutex::const_new(false);
pub async fn stop_head_shake_detection() {
    if *GESTURE_DETECTION_RUNNING.lock().await {
        *ABORT_GESTURE_DETECTION.lock().await = true;
    }
}
pub async fn start_head_shake_detection() {
    if *OXR_STATE.lock().await == VRStatus::Initialized {
        let frame_time = (1000.
            / OXR_HANDLE
                .get()
                .unwrap()
                .lock()
                .await
                .current_refresh_rate() as f32) as u64;
        tokio::task::spawn(async move {
            *GESTURE_DETECTION_RUNNING.lock().await = true;
            *ABORT_GESTURE_DETECTION.lock().await = false;
            loop {
                if *ABORT_GESTURE_DETECTION.lock().await {
                    break;
                }
                let mut ctx = OXR_HANDLE.get().as_ref().unwrap().lock().await;
                let _ = ctx.run();
                if let Some(handler) = INPUT_CONTEXT.lock().await.as_mut() {
                    if check_user_activity(&mut handler.1).unwrap() {
                        log::info!("button press detected");
                        send_event("GESTURE_DETECTED", "").await;
                        break;
                    }
                }
                if *OXR_STATE.lock().await == VRStatus::Initialized {
                    if let Some(posef) = get_pose("head shake", &mut *ctx).await {
                        let pos = posef.position;
                        let quat = posef.orientation;
                        GESTURE_DETECTOR
                            .lock()
                            .await
                            .log_pose([pos.x, pos.y, pos.z], [quat.x, quat.y, quat.z, quat.w])
                            .await;
                    }
                } else {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(frame_time)).await;
            }
            *GESTURE_DETECTION_RUNNING.lock().await = false;
        });
    }
}

pub async fn set_brightness(brightness: f64, perceived_brightness_adjustment_gamma: Option<f64>) {
    if *OXR_STATE.lock().await != VRStatus::Initialized {
        return;
    }
    let mut brightness = brightness.clamp(0.0, 1.0);
    // Adjust the brightness value for perceived brightness
    if let Some(gamma) = perceived_brightness_adjustment_gamma {
        brightness = adjust_for_perceived_brightness(brightness, gamma);
    }
    let brightness = ((1. - brightness) * 255.) as u8;

    let mut ctx = OXR_HANDLE.wait().lock().await;
    let overlay_handle = *OXR_BRIGHTNES_OVERLAY_HANDLE.lock().await;
    if overlay_handle.is_none() {
        return;
    }

    ctx.set_raw_texture(
        overlay_handle.unwrap(),
        // RgbaTexture::new(1, 1, [brightness, 0, 0, 255].to_vec()),
        RgbaTexture::new(1, 1, [0, 0, 0, brightness].to_vec()),
        false,
    );
    let _ = ctx.run();
}

fn adjust_for_perceived_brightness(linear_percent: f64, gamma: f64) -> f64 {
    linear_percent.powf(1.0 / gamma)
}
pub static SLEEP_DETECTOR: LazyLock<Mutex<SleepDetector>> =
    LazyLock::new(|| Mutex::new(SleepDetector::new()));
pub static GESTURE_DETECTOR: LazyLock<Mutex<GestureDetector>> =
    LazyLock::new(|| Mutex::new(GestureDetector::new()));
