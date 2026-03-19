use std::sync::{Arc, atomic::{AtomicBool, AtomicU8, Ordering}};

use gmodx::lua::{self, UserDataRef};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::socket::handler;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SocketState {
    Connected = 0,
    Connecting = 1,
    NotConnected = 2,
    Disconnected = 3,
}

pub struct AtomicState(AtomicU8);

impl AtomicState {
    pub const fn new(state: SocketState) -> Self {
        Self(AtomicU8::new(state as u8))
    }

    pub fn get(&self) -> SocketState {
        match self.0.load(Ordering::Acquire) {
            0 => SocketState::Connected,
            1 => SocketState::Connecting,
            2 => SocketState::NotConnected,
            3 => SocketState::Disconnected,
            _ => unreachable!(),
        }
    }

    pub fn set(&self, state: SocketState) {
        self.0.store(state as u8, Ordering::Release);
    }
}

#[derive(Debug)]
pub enum SocketCommand {
    Send(String),
    Close,
    CloseNow,
}

pub struct SocketMetadata {
    pub id: Uuid,
    pub state: AtomicState,
    pub url: String,
    disconnect_notified: AtomicBool,
}

impl SocketMetadata {
    pub fn new(url: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            state: AtomicState::new(SocketState::Connecting),
            url,
            disconnect_notified: AtomicBool::new(false),
        }
    }

    pub fn reset_for_open(&self) {
        self.state.set(SocketState::Connecting);
        self.disconnect_notified.store(false, Ordering::Release);
    }

    pub fn mark_disconnect_notified(&self) -> bool {
        !self.disconnect_notified.swap(true, Ordering::AcqRel)
    }
}

pub struct Socket {
    pub meta: Arc<SocketMetadata>,
    pub sender: mpsc::UnboundedSender<SocketCommand>,
}

impl Socket {
    pub fn new(l: &lua::State, url: lua::String) -> lua::Result<UserDataRef<Socket>> {
        let meta = Arc::new(SocketMetadata::new(url.to_string()));
        let (sender, _) = mpsc::unbounded_channel();

        let ud = l.create_userdata(Self {
            meta: Arc::clone(&meta),
            sender,
        });

        handler::register(meta.id, ud.as_any().clone());
        Self::spawn_for_userdata(&ud)?;
        Ok(ud)
    }

    pub fn spawn_for_userdata(ud: &UserDataRef<Socket>) -> lua::Result<()> {
        let mut socket = ud.borrow_mut();
        socket.meta.reset_for_open();
        let sender = handler::spawn(socket.meta.id, Arc::clone(&socket.meta));
        handler::set_sender(socket.meta.id, sender.clone());
        socket.sender = sender;
        Ok(())
    }

    pub fn reopen(l: &lua::State, ud: &UserDataRef<Socket>) -> lua::Result<bool> {
        if ud.borrow().meta.state.get() != SocketState::Disconnected {
            return Ok(true);
        }

        let _ = l;
        Self::spawn_for_userdata(ud)?;
        Ok(true)
    }

    pub fn notify_disconnect_now(meta: Arc<SocketMetadata>, reason: String) {
        meta.state.set(SocketState::Disconnected);
        if meta.mark_disconnect_notified() {
            handler::queue_disconnect(meta.id, reason);
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        handler::unregister(self.meta.id);
        let _ = self.sender.send(SocketCommand::CloseNow);
    }
}

impl std::fmt::Display for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tungstenite ({})", self.meta.id)
    }
}
