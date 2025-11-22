pub mod functions;

use gmodx::*;
use functions::lua_tungstenite;

#[gmod13_open]
fn gmod13_open(l: lua::State) {
    let t_tung = l.create_table();
    t_tung.set(&l, "connect", l.create_function(lua_tungstenite::connect)).unwrap();

    l.set_global("tungstenite", t_tung).expect("failed to set 'tungstenite' global");
}