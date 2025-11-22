pub mod lua_tungstenite {
    use std::{cell::RefCell, sync::{Arc, Mutex, mpsc}};

    use gmodx::lua::{self, ObjectLike, UserDataRef};
    use tungstenite::Message;

    #[derive(Debug)]
    pub enum DataType {
        Generic,
        Error,
        Disconnect,
    }

    #[derive(Debug)]
    pub struct RustChannel {
        pub data_type: DataType,
        pub data: String
    }
    #[derive(Debug)]
    pub struct LuaChannel {
        pub data_type: DataType,
        pub data: String
    }

    pub struct Socket {
        tx: mpsc::Sender<RustChannel>,
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
            .send(RustChannel { data_type: DataType::Generic, data: data.to_string() })
            .map_err(|e| lua::Error::Runtime(format!("failed to send a message ({e})")))?;

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
                                    func.call_no_rets_logged(l, (mt, message.data))
                                        .unwrap();
                                }
                            },
                            DataType::Error => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_error") {
                                    func.call_no_rets_logged(l, (mt, message.data))
                                        .unwrap();
                                }
                            },
                            DataType::Disconnect => {
                                if let Ok(func) = mt.get::<lua::Function>(l, "on_disconnect") {
                                    func.call_no_rets_logged(l, (mt, message.data))
                                        .unwrap();
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

        let (tx_to_thread, rx_from_lua) = mpsc::channel::<RustChannel>();
        let (tx_to_lua, rx_to_lua) = mpsc::channel::<LuaChannel>();

        let rx_to_lua_arc = Arc::new(Mutex::new(rx_to_lua));

        std::thread::spawn(move || {
            let (mut socket, _) = match tungstenite::connect(url) {
                Ok(res) => res,
                Err(err) => {
                    let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Error, data: err.to_string() });
                    return;
                }
            };

            loop {
                match rx_from_lua.try_recv() {
                    Ok(msg) => {
                        tracing::debug!("  lua->rust: {}", msg.data);
                        let _ = socket.send(Message::text(msg.data));
                    }
                    Err(mpsc::TryRecvError::Empty) => {},
                    Err(_) => break,
                }

                match socket.read() {
                    Ok(Message::Text(text)) => {
                        tracing::debug!("  rust->lua: {}", text.to_string());
                        let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Generic, data: text.to_string() });
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = socket.send(Message::Pong(p));
                    }
                    Err(e) => {
                        let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Error, data: e.to_string() });
                        break;
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Disconnect, data: frame.reason.to_string() });
                        } else {
                            let _ = tx_to_lua.send(LuaChannel { data_type: DataType::Disconnect, data: "unknown".into() });
                        }
                    },
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