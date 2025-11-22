pub mod lua_tungstenite {
    use std::{cell::RefCell, sync::{Arc, Mutex, mpsc}};

    use gmodx::lua::{self, ObjectLike, UserDataRef};
    use tungstenite::Message;

    #[derive(Debug)]
    pub enum DataType {
        Generic,
        Error,
        Disconnect,
        Connect,
    }

    #[derive(Debug)]
    pub struct LuaChannel {
        pub data_type: DataType,
        pub data: String
    }

    pub struct Socket {
        tx: mpsc::Sender<String>,
        rx: Arc<Mutex<mpsc::Receiver<LuaChannel>>>,

        id: uuid::Uuid
    }

    impl lua::UserData for Socket {
        fn methods(methods: &mut lua::Methods) {
            methods.add(c"send", send);

            // somewhat compatibility layer with gwsockets
            methods.add(c"write", send);
        }
        fn meta_methods(methods: &mut lua::Methods) {
            methods.add(c"__tostring", |_l: &lua::State, this: UserDataRef<Socket>| {
                format!("tungstenite ({})", this.borrow().id.to_string())
            });
        }

        fn name() -> &'static str { "tungstenite" }
    }

    thread_local! {
        static SOCKETS: RefCell<Vec<lua::UserDataRef<Socket>>> = RefCell::new(Vec::new());
    }

    // @note: metatable functions
    pub fn send(_l: &lua::State, this: lua::UserDataRef<Socket>, data: lua::String) -> lua::Result<()> {
        let ud = this.borrow();

        ud.tx
            .send(data.to_string())
            .map_err(|e| lua::Error::Runtime(format!("send failed: {e}")))?;

        Ok(())
    }

    // @note: api functions
    fn run_callbacks(l: &lua::State) -> lua::Result<()> {
        SOCKETS.with(|s| {
            for ud_ref in s.borrow_mut().iter() {
                let ud = ud_ref.borrow();
                let mt = ud_ref.as_any();

                let rx = &ud.rx;

                match rx.lock().unwrap().try_recv() {
                    Ok(message) => {
                        match message.data_type {
                            DataType::Generic => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_message") {
                                    if let Err(e) = func.call_no_rets(l, (mt, message.data)) {
                                        l.error_no_halt_with_stack(&e.to_string());
                                    }
                                }
                            },
                            DataType::Error => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_error") {
                                    if let Err(e) = func.call_no_rets(l, (mt, message.data)) {
                                        l.error_no_halt_with_stack(&e.to_string());
                                    }
                                }
                            },
                            DataType::Connect => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_connect") {
                                    if let Err(e) = func.call_no_rets(l, (mt, message.data)) {
                                        l.error_no_halt_with_stack(&e.to_string());
                                    }
                                }
                            },
                            DataType::Disconnect => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_disconnect") {
                                    if let Err(e) = func.call_no_rets(l, (mt, message.data)) {
                                        l.error_no_halt_with_stack(&e.to_string());
                                    }
                                }
                            }
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => continue
                }
            }
        });

        Ok(())
    }

    pub fn connect(l: &lua::State, url: lua::String) -> lua::Result<lua::UserDataRef<Socket>> {
        let url = url.to_string();

        let (tx_to_thread, rx_from_lua) = mpsc::channel::<String>();
        let (tx_to_lua, rx_to_lua) = mpsc::channel::<LuaChannel>();

        let rx_to_lua_arc = Arc::new(Mutex::new(rx_to_lua));

        std::thread::spawn(move || {
            let (mut socket, _) = match tungstenite::connect(url) {
                Ok(res) => {
                    let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Connect, data: String::default() });
                    res
                },
                Err(err) => {
                    let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Error, data: err.to_string() });
                    return;
                }
            };

            match &mut socket.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(tcp) => {
                    tcp.set_nonblocking(true)
                        .unwrap(); // @note: hopium on maximum that it won't ever backfire
                },
                tungstenite::stream::MaybeTlsStream::NativeTls(tls_stream) => {
                    tls_stream.get_mut().set_nonblocking(true)
                        .unwrap();
                }
                _ => {}
            }

            loop {
                match rx_from_lua.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(message) => {
                        let _ = socket.send(Message::text(message));
                    },
                    Err(mpsc::RecvTimeoutError::Timeout) => {},
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                match socket.read() {
                    Ok(Message::Text(text)) => {
                        let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Generic, data: text.to_string() });
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = socket.send(Message::Pong(p));
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Disconnect, data: frame.reason.to_string() });
                        } else {
                            let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Disconnect, data: "unknown".into() });
                        }
                    },
                    Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {},
                    Err(e) => {
                        let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Error, data: e.to_string() });
                        break;
                    }
                    _ => {},
                }
            }
        });

        let ud = l.create_userdata(Socket {
            tx: tx_to_thread,
            rx: rx_to_lua_arc,

            id: uuid::Uuid::new_v4()
        });

        SOCKETS.with(|c| c.borrow_mut().push(ud.clone()));

        l.globals()
            .get::<lua::Table>(l, "timer")?
            .get::<lua::Function>(l, "Create")?
            .call_no_rets(l, ("tungstenite.callbacks", 0.0, 0.0, l.create_function(run_callbacks)))?;

        Ok(ud)
    }
}