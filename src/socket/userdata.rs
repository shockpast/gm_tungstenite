use gmodx::lua::{self, Methods, State, UserData, UserDataRef};

use crate::socket::types::{Socket, SocketCommand, SocketState};

fn send(_l: &State, this: UserDataRef<Socket>, data: lua::String) -> lua::Result<()> {
    let socket = this.borrow();

    socket
        .sender
        .send(SocketCommand::Send(data.to_string()))
        .map_err(|err| lua::Error::Runtime(format!("send failed: {err}").into()))?;

    Ok(())
}

fn close(_l: &State, this: UserDataRef<Socket>) -> lua::Result<()> {
    let socket = this.borrow();
    socket.meta.state.set(SocketState::Disconnected);

    socket
        .sender
        .send(SocketCommand::Close)
        .map_err(|err| lua::Error::Runtime(format!("failed to close connection ({err})").into()))?;

    Ok(())
}

fn close_now(_: &State, this: UserDataRef<Socket>) -> lua::Result<()> {
    let socket = this.borrow();

    socket
        .sender
        .send(SocketCommand::CloseNow)
        .map_err(|err| lua::Error::Runtime(format!("failed to close connection ({err})").into()))?;

    Socket::notify_disconnect_now(socket.meta.clone(), "closed by user".to_string());
    Ok(())
}

fn open(l: &State, this: UserDataRef<Socket>) -> lua::Result<bool> {
    Socket::reopen(l, &this)
}

impl UserData for Socket {
    fn methods(m: &mut Methods) {
        m.add(c"send", send);
        m.add(c"write", send);
        m.add(c"close", close);
        m.add(c"close_now", close_now);
        m.add(c"closeNow", close_now);
        m.add(c"open", open);
    }

    fn meta_methods(m: &mut Methods) {
        m.add(c"__tostring", |_: &State, sock: UserDataRef<Socket>| {
            sock.borrow().to_string()
        });
    }

    fn name() -> &'static str {
        "tungstenite"
    }
}
