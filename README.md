# 🪨 gmsv_tungstenite

websocket client for garry's mod servers, inspired by [FredyH/GWSockets](https://github.com/FredyH/GWSockets)

## examples

you can find examples to how interact with this library in [examples/](examples/) folder

## documentation

### `tungstenite.connect(string url) -> tungstenite`

connects to a websocket server and returns an instance of tungstenite, otherwise errors

### `tungstenite:send(string data)`

sends data to server, returns nothing, errors if failed to send (can happen if socket was closed)

### `tungstenite:write(string data)`

alias of `tungstenite:send`

### `tungstenite:close()`

closes connection and waits until all messages in queue are received/sent

### `tungstenite:close_now()`

forcefully closes connection and discards receive/send queue

## callbacks

### `tungstenite:on_message(string data)`

### `tungstenite:on_error(string message)`

### `tungstenite:on_disconnect(string message)`

whenever `on_disconnect` is called, socket should be considered as closed

### `tungstenite:on_connect()`