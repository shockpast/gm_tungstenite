mod handler;
mod types;
mod userdata;

use gmodx::lua::{State, Table};

use crate::socket::handler::shutdown_all;

pub fn on_gmod_open(state: &State, table: &Table) {
    table.raw_set(state, "connect", state.create_function(|l: &State, url| {
        crate::socket::types::Socket::new(l, url)
    }));
}

pub fn on_gmod_close() {
    shutdown_all();
}
