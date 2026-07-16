pub mod commands;
pub mod elevation;
pub mod linux;
mod models;
mod notifications;
use dbus::blocking::Connection;
use log::{debug, error};
use notify::event::{AccessKind, AccessMode, RemoveKind};
use notify::{Event, EventHandler, Watcher};
use oyasumi_shared::RESOURCES_PATH;
use rodio::{DeviceSinkBuilder, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Sender;

use crate::os::models::NewSound;
use crate::utils::send_event;
type PlaySoundSender = LazyLock<Mutex<Option<Sender<(String, f32)>>>>;
pub static DBUS_CONNECTION: Mutex<Option<Connection>> = Mutex::const_new(None);
pub async fn connect_dbus() -> bool {
    let mut lock = DBUS_CONNECTION.lock().await;
    if lock.is_some() {
        return true;
    }
    match Connection::new_session() {
        Ok(v) => {
            lock.replace(v);
            debug!("[core] connected to dbus");
            true
        }
        Err(err) => {
            error!("[core] failed to coonect to dbus: {:?}", err);
            false
        }
    }
}
static PLAY_SOUND_TX: PlaySoundSender = LazyLock::new(Mutex::default);
async fn process_new_sound_files(
    sounds: &Arc<Mutex<HashMap<String, Box<[u8]>>>>,
    paths: impl IntoIterator<Item = PathBuf>,
) {
    let mut guard = sounds.lock().await;
    let mut sounds_ = Vec::new();
    for path in paths {
        log::debug!("process_new_sound_files: {:?}",path);
        if path.file_name().is_none() {
            continue;
        }
        if path.file_name().unwrap().to_string_lossy().ends_with(".md")
            || path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".gitignore")
        {
            continue;
        }
        let mut file = match File::open(path.clone()) {
            Ok(f) => f,
            Err(e) => {
                error!("[Core] Failed to open sound file: {}", e);
                return;
            }
        };
        let mut buff = Vec::new();
        if let Err(err) = file.read_to_end(&mut buff) {
            error!("failed to read audio file {:#?} {:?}", path, err);
        }
        let buff = buff.into_boxed_slice();
        let sound = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take_while(|x| *x != '.')
            .collect::<String>();
        log::info!("loading sound: {} ({})", sound, path.display());
        sounds_.push(NewSound {
            name: sound.clone(),
            duration_ms: rodio::Decoder::new(Cursor::new(buff.clone()))
                .unwrap()
                .total_duration()
                .unwrap_or_default()
                .as_millis() as u32,
        });
        guard.insert(sound, buff);
    }
    send_event("NEW_SOUNDS", sounds_).await;
}
pub async fn init_sound_playback() {
    // Create channels
    let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel::<(String, f32)>(32);
    // let (std_tx, std_rx) = std::sync::mpsc::channel::<(String, f32)>();

    // Store the tokio sender
    *PLAY_SOUND_TX.lock().await = Some(tokio_tx);

    // Spawn standard thread to play sounds
    tokio::task::spawn(async move {
        // Load sound files
        let sounds: Arc<Mutex<HashMap<String, Box<[u8]>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let stream_handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink) => sink,
            Err(e) => {
                error!("[Core] Failed to initialize audio output stream: {}", e);
                return;
            }
        };
        process_new_sound_files(
            &sounds,
            RESOURCES_PATH
                .join("sounds")
                .read_dir()
                .unwrap()
                .into_iter()
                .filter_map(|r| match r {
                    Ok(v) => Some(v.path()),
                    Err(err) => {
                        log::error!("{:?}", err);
                        None
                    }
                }),
        ).await;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Result<notify::Event>>(10);
        let mut watcher = notify::recommended_watcher(Yourmom { inner: tx }).unwrap();
        watcher
            .watch(
                &RESOURCES_PATH.join("sounds"),
                notify::RecursiveMode::NonRecursive,
            )
            .unwrap();
        let sounds2 = sounds.clone();
        tokio::task::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    Ok(v) => match v.kind {
                        notify::EventKind::Access(access_kind) => {
                            if let AccessKind::Close(AccessMode::Write) = access_kind {
                                process_new_sound_files(&sounds2, v.paths).await;
                            }
                        }
                        notify::EventKind::Remove(remove_kind) => {
                            if remove_kind == RemoveKind::File {
                                for path in v.paths {
                                    let sound = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .chars()
                                        .take_while(|x| *x != '.')
                                        .collect::<String>();
                                    let mut guard = sounds2.lock().await;
                                    if guard.contains_key(&sound) {
                                        guard.remove(&sound);
                                        send_event("REMOVE_SOUND", sound).await;
                                    } else {
                                        log::warn!("removed unknown sound:{} ({:#?})", sound, path);
                                    }
                                }
                            }
                        }
                        _ => continue,
                    },
                    Err(err) => log::error!("watch error: {:?}", err),
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        stream_handle.pause();
        // Initialize output stream
        let player = rodio::Player::connect_new(stream_handle.mixer());
        // Play sounds when requested
        while let Some((sound, volume)) = tokio_rx.recv().await {
            if let Some(source) = sounds.lock().await.get(&sound) {
                stream_handle.play();
                // Play sound
                let decoder = rodio::Decoder::new(Cursor::new(source.clone())).unwrap();
                decoder.total_duration();
                player.append(decoder);

                player.set_volume(volume);
                player.sleep_until_end();
                stream_handle.pause();
            } else {
                error!("[Core] Sound not found: {}", sound);
            }
        }
    });
}
pub struct Yourmom {
    inner: tokio::sync::mpsc::Sender<notify::Result<Event>>,
}
impl EventHandler for Yourmom {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        let _ = self.inner.send(event);
    }
}
