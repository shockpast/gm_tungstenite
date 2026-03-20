use std::sync::Arc;
use std::sync::{LazyLock, Mutex};

use futures_util::{SinkExt, StreamExt};
use gmodx::{is_closed, next_tick, lua::{AnyUserData, Function, ObjectLike}, tokio_tasks};
use tokio::sync::mpsc;
use tokio::time::{self, Duration, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::{self, Message, Utf8Bytes, protocol::CloseFrame}};
use uuid::Uuid;

use crate::socket::types::{SocketCommand, SocketMetadata, SocketState};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

struct SocketRegistryEntry {
    target: AnyUserData,
    sender: Option<mpsc::UnboundedSender<SocketCommand>>,
}

static SOCKETS: LazyLock<Mutex<std::collections::HashMap<Uuid, SocketRegistryEntry>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub fn register(id: Uuid, target: AnyUserData) {
    SOCKETS.lock().unwrap().insert(
        id,
        SocketRegistryEntry { target, sender: None },
    );
}

pub fn set_sender(id: Uuid, sender: mpsc::UnboundedSender<SocketCommand>) {
    if let Some(entry) = SOCKETS.lock().unwrap().get_mut(&id) {
        entry.sender = Some(sender);
    }
}

pub fn unregister(id: Uuid) {
    SOCKETS.lock().unwrap().remove(&id);
}

pub fn shutdown_all() {
    let mut sockets = SOCKETS.lock().unwrap();
    for entry in sockets.values() {
        if let Some(sender) = &entry.sender {
            let _ = sender.send(SocketCommand::CloseNow);
        }
    }

    sockets.clear();
}

fn get_target(id: Uuid) -> Option<AnyUserData> {
    SOCKETS.lock().unwrap().get(&id).map(|entry| entry.target.clone())
}

fn queue_callback(id: Uuid, name: &'static str, data: Option<String>) {
    next_tick(move |l| {
        if is_closed() {
            return;
        }

        let Some(target) = get_target(id) else {
            return;
        };

        let func = match target.get::<Function>(l, name) {
            Ok(func) => func,
            Err(_) => return,
        };

        let result = match data {
            Some(data) => func.call::<()>(l, (target.clone(), data)),
            None => func.call::<()>(l, target.clone()),
        };

        if let Err(err) = result {
            l.error_no_halt_with_stack(&err.to_string());
        }
    });
}

pub fn queue_disconnect(id: Uuid, reason: String) {
    queue_callback(id, "on_disconnect", Some(reason));
}

fn notify_disconnect(meta: &Arc<SocketMetadata>, reason: impl Into<String>) {
    meta.state.set(SocketState::Disconnected);
    if meta.mark_disconnect_notified() {
        queue_disconnect(meta.id, reason.into());
    }
}

pub fn spawn(id: Uuid, meta: Arc<SocketMetadata>) -> mpsc::UnboundedSender<SocketCommand> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio_tasks::spawn(handle_socket(receiver, id, meta));

    sender
}

async fn handle_socket(mut receiver: mpsc::UnboundedReceiver<SocketCommand>, id: Uuid, meta: Arc<SocketMetadata>) {
    let connection = connect_async(meta.url.as_str()).await;
    let (stream, _) = match connection {
        Ok(parts) => parts,
        Err(err) => {
            notify_disconnect(&meta, format!("connect failed: {err}"));
            queue_callback(id, "on_error", Some(err.to_string()));
            return;
        }
    };

    meta.state.set(SocketState::Connected);
    queue_callback(id, "on_connect", None);

    let (mut writer, mut reader) = stream.split();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    heartbeat.tick().await;

    let disconnect_reason = loop {
        tokio::select! {
            maybe_command = receiver.recv() => {
                let Some(command) = maybe_command else {
                    break "socket command channel closed".to_string();
                };

                match command {
                    SocketCommand::Send(data) => {
                        if let Err(err) = writer.send(Message::Text(data.into())).await {
                            queue_callback(id, "on_error", Some(err.to_string()));
                            break format!("write failed: {err}");
                        }
                    }
                    SocketCommand::Close => {
                        let frame = CloseFrame {
                            code: tungstenite::protocol::frame::coding::CloseCode::Normal,
                            reason: Utf8Bytes::from_static("closed by user"),
                        };

                        if let Err(err) = writer.send(Message::Close(Some(frame))).await {
                            queue_callback(id, "on_error", Some(err.to_string()));
                            break format!("close failed: {err}");
                        }

                        break "closed by user".to_string();
                    }
                    SocketCommand::CloseNow => {
                        let _ = writer.close().await;
                        break "closed by user".to_string();
                    }
                }
            }
            maybe_message = reader.next() => {
                let Some(message) = maybe_message else {
                    break "connection closed".to_string();
                };

                match message {
                    Ok(Message::Text(text)) => {
                        queue_callback(id, "on_message", Some(text.to_string()));
                    }
                    Ok(Message::Binary(data)) => {
                        queue_callback(id, "on_message", Some(String::from_utf8_lossy(&data).to_string()));
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(err) = writer.send(Message::Pong(data)).await {
                            queue_callback(id, "on_error", Some(err.to_string()));
                            break format!("pong failed: {err}");
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        break frame
                            .map(|frame| frame.reason.to_string())
                            .filter(|reason| !reason.is_empty())
                            .unwrap_or_else(|| "connection closed".to_string());
                    }
                    Err(err) => {
                        queue_callback(id, "on_error", Some(err.to_string()));
                        break format!("read failed: {err}");
                    }
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                if let Err(err) = writer.send(Message::Ping(Vec::new().into())).await {
                    queue_callback(id, "on_error", Some(err.to_string()));
                    break format!("heartbeat failed: {err}");
                }
            }
        }
    };

    notify_disconnect(&meta, disconnect_reason);
}
