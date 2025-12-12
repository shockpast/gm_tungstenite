pub mod lua_tungstenite {
    use std::{cell::RefCell, sync::{Arc, Mutex, Once, mpsc}};

    use gmodx::{bstr, lua::{self, ObjectLike, UserDataRef}};
    use tungstenite::{Message, Utf8Bytes, protocol::{CloseFrame, frame::coding::CloseCode}};

    #[derive(Debug)]
    pub enum LuaMessageType {
        Message,
        Error,
        Disconnect,
        Connect,
    }
    #[derive(Debug)]
    pub enum RustMessageType {
        Message,
        Close,
    }

    #[derive(Debug)]
    pub struct LuaChannel {
        pub message_type: LuaMessageType,
        pub data: Option<String>
    }
    #[derive(Debug)]
    pub struct RustChannel {
        pub message_type: RustMessageType,
        pub data: Option<String>
    }

    pub struct Socket {
        tx: mpsc::Sender<RustChannel>,
        rx: Arc<Mutex<mpsc::Receiver<LuaChannel>>>,

        id: uuid::Uuid,
        closed: bool,
        url: String,
    }

    impl lua::UserData for Socket {
        fn methods(methods: &mut lua::Methods) {
            methods.add(c"send", send);
            methods.add(c"close", close);
            methods.add(c"close_now", close_now);
            methods.add(c"open", open);
            
            // @note: somewhat compatibility layer with gwsockets
            methods.add(c"write", send);
            methods.add(c"closeNow", close_now);
        }
        fn meta_methods(methods: &mut lua::Methods) {
            methods.add(c"__tostring", |_l: &lua::State, this: UserDataRef<Socket>| {
                format!("tungstenite ({})", this.borrow().id)
            });
            methods.add(c"__gc", |l: &lua::State, this: UserDataRef<Socket>| -> lua::Result<()> {
                SOCKETS.with(|c| c.borrow_mut().retain(|s| !std::ptr::eq(s, &this)));
                close_now(l, this)?;

                Ok(())
            })
        }

        fn name() -> &'static str { "tungstenite" }
    }

    thread_local! {
        static SOCKETS: RefCell<Vec<lua::UserDataRef<Socket>>> = RefCell::new(Vec::new());
    }

    static CALLBACKS: Once = Once::new();

    fn spawn(url: String, tx_to_lua: mpsc::Sender<LuaChannel>, rx_from_lua: mpsc::Receiver<RustChannel>) {
        std::thread::spawn(move || {
            let (mut socket, _) = match tungstenite::connect(&url) {
                Ok(res) => {
                    let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Connect, data: None });
                    res
                },
                Err(err) => {
                    let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Error, data: Some(err.to_string()) });
                    return;
                }
            };

            match &mut socket.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(tcp) => {
                    tcp.set_nodelay(true)
                        .unwrap();
                    tcp.set_nonblocking(true)
                        .unwrap(); // @note: hopium on maximum that it won't ever backfire
                },
                tungstenite::stream::MaybeTlsStream::NativeTls(tls_stream) => {
                    let stream = tls_stream.get_mut();

                    stream.set_nodelay(true)
                        .unwrap();
                    stream.set_nonblocking(true)
                        .unwrap();
                }
                _ => {}
            }

            loop {
                match rx_from_lua.try_recv() {
                    Ok(message) => {
                        match message.message_type {
                            RustMessageType::Message => {
                                if let Some(ref text) = message.data {
                                    let _ = socket.send(Message::text(text));
                                }
                            },
                            RustMessageType::Close => {
                                let _ = socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: Utf8Bytes::from_static("unknown") }));
                            },
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => {},
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }

                match socket.read() {
                    Ok(Message::Text(text)) => {
                        let utext = match std::str::from_utf8(text.as_bytes()) {
                            Ok(s) => s.to_string(),
                            Err(_) => String::from_utf8_lossy(text.as_bytes()).to_string()
                        };

                        let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Message, data: Some(utext) });
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = socket.send(Message::Pong(p));
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Disconnect, data: Some(frame.reason.to_string()) });
                        } else {
                            let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Disconnect, data: Some("unknown".to_string()) });
                        }
                    },
                    Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {},
                    Err(e) => {
                        let _ = tx_to_lua.send(LuaChannel { message_type: LuaMessageType::Error, data: Some(e.to_string()) });
                        break;
                    }
                    _ => {},
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
    }

    // @note: metatable functions
    pub fn send(_l: &lua::State, this: lua::UserDataRef<Socket>, data: lua::String) -> lua::Result<()> {
        let ud = this.borrow();

        ud.tx
            .send(RustChannel { message_type: RustMessageType::Message, data: Some(data.to_string()) })
            .map_err(|e| lua::Error::Runtime(format!("send failed: {e}")))?;

        Ok(())
    }
    pub fn close(_l: &lua::State, this: lua::UserDataRef<Socket>) -> lua::Result<()> {
        let mut ud = this.borrow_mut();
        ud.closed = true;

        ud.tx
            .send(RustChannel { message_type: RustMessageType::Close, data: None })
            .map_err(|e| lua::Error::Runtime(format!("failed to close connection ({e})")))?;

        Ok(())
    }
    pub fn close_now(l: &lua::State, this: lua::UserDataRef<Socket>) -> lua::Result<()> {
        let mut ud = this.borrow_mut();
        let mt = this.as_any();

        ud.closed = true;
        ud.tx = mpsc::channel().0;
        ud.rx = Arc::new(Mutex::new(mpsc::channel().1));

        SOCKETS.with(|c| c.borrow_mut().retain(|s| !std::ptr::eq(s, &this)));

        if let Ok(func) = mt.get::<lua::Function>(l, "on_disconnect") {
            func.call_no_rets_logged(l, (mt, "closed by user"))?;
        }

        Ok(())
    }
    pub fn open(l: &lua::State, this: lua::UserDataRef<Socket>) -> lua::Result<bool> {
        {
            let ud = this.borrow();
            if !ud.closed {
                return Ok(true);
            }
        }

        let url = {
            let ud = this.borrow();
            ud.url.clone()
        };

        let (tx_to_thread, rx_from_lua) = mpsc::channel::<RustChannel>();
        let (tx_to_lua, rx_to_lua) = mpsc::channel::<LuaChannel>();

        let rx_to_lua_arc = Arc::new(Mutex::new(rx_to_lua));

        spawn(url, tx_to_lua, rx_from_lua);

        {
            let mut ud = this.borrow_mut();
            ud.tx = tx_to_thread;
            ud.rx = rx_to_lua_arc;
            ud.closed = false;
        }

        SOCKETS.with(|c| {
            if !c.borrow().iter().any(|s| std::ptr::eq(s, &this)) {
                c.borrow_mut().push(this.clone());
            }
        });
        CALLBACKS.call_once(|| init(l).expect("failed to create a run_callbacks timer"));

        Ok(true)
    }

    // @note: api functions
    fn run_callbacks(l: &lua::State) -> lua::Result<()> {
        SOCKETS.with(|s| {
            s.borrow_mut().retain(|ud_ref| {
                let mt = ud_ref.as_any();

                let rx = {
                    let ud = ud_ref.borrow();
                    ud.rx.clone()
                };

                if let Ok(receiver) = rx.lock() {
                    match receiver.try_recv() {
                        Ok(message) => {
                            match message.message_type {
                                LuaMessageType::Connect
                                | LuaMessageType::Message
                                | LuaMessageType::Error => {
                                    let key = match message.message_type {
                                        LuaMessageType::Connect => "on_connect",
                                        LuaMessageType::Message => "on_message",
                                        LuaMessageType::Error => "on_error",
                                        _ => unreachable!()
                                    };

                                    let _ = mt.get::<lua::Function>(l, key)
                                        .and_then(|func| func.call_no_rets(l, (mt, message.data)))
                                        .map_err(|e| l.error_no_halt_with_stack(&e.to_string()));
                                },
                                LuaMessageType::Disconnect => {
                                    if let Ok(func) = mt.get::<lua::Function>(l, "on_disconnect") {
                                        {
                                            let mut ud_mut = ud_ref.borrow_mut();
                                            ud_mut.closed = true;
                                        }

                                        if let Err(e) = func.call_no_rets(l, (mt, message.data)) {
                                            l.error_no_halt_with_stack(&e.to_string());
                                        }
                                    }

                                    return false;
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) => { return false },
                        Err(mpsc::TryRecvError::Empty) => {},
                    };

                    true
                } else {
                    false
                }
            })
        });

        Ok(())
    }

    fn init(l: &lua::State) -> lua::Result<()> {
        l.globals()
            .get::<lua::Table>(l, "timer")?
            .get::<lua::Function>(l, "Create")?
            .call_no_rets_logged(l, ("tungstenite.callbacks", 0.0, 0.0, l.create_function(run_callbacks)))?;

        Ok(())
    }

    pub fn connect(l: &lua::State, url: lua::String) -> lua::Result<lua::UserDataRef<Socket>> {
        let url = url.to_string();

        let (tx_to_thread, rx_from_lua) = mpsc::channel::<RustChannel>();
        let (tx_to_lua, rx_to_lua) = mpsc::channel::<LuaChannel>();

        let rx_to_lua_arc = Arc::new(Mutex::new(rx_to_lua));

        spawn(url.clone(), tx_to_lua, rx_from_lua);

        let ud = l.create_userdata(Socket {
            tx: tx_to_thread,
            rx: rx_to_lua_arc,

            id: uuid::Uuid::new_v4(),
            closed: false,
            url: url.clone(),
        });

        SOCKETS.with(|c| c.borrow_mut().push(ud.clone()));
        CALLBACKS.call_once(|| init(l).expect("failed to create a run_callbacks timer"));

        Ok(ud)
    }
}