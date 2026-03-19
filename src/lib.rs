pub mod config;
pub mod socket;

use gmodx::{gmod13_close, gmod13_open, lua::State};

use crate::config::VERSION;

#[gmod13_open]
fn gmod13_open(state: State) {
    let table = state.create_table();
    table.raw_set(&state, "VERSION", VERSION);

    //
    socket::on_gmod_open(&state, &table);

    //
    state
        .set_global("tungstenite", table)
        .expect("failed to set 'tungstenite' global");
}

#[gmod13_close]
fn gmod13_close(_l: State) {
    socket::on_gmod_close();
}
