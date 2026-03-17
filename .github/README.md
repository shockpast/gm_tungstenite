# 🪨 gmsv_tungstenite

websocket client for garry's mod servers, inspired by [FredyH/GWSockets](https://github.com/FredyH/GWSockets)

## installation

place `gmsv_tungstenite.dll` (windows) or `gmsv_tungstenite_linux64.dll` (linux) into your garry's mod servers `garrysmod/lua/bin` folder

## examples

you can find examples to how interact with this library in [examples/](/examples/) folder

## basic usage

```lua
require("tungstenite")

-- connect to a websocket server
local ws = tungstenite.connect("wss://example.com/ws")

-- callback: connection established
function ws:on_connect()
    print("Connected!")
    ws:send("Hello, server!")
end

-- callback: received message from server
function ws:on_message(message)
    print("Received:", message)
end

-- callback: error occurred
function ws:on_error(err)
    print("Error:", err)
end

-- callback: disconnected
function ws:on_disconnect(reason)
    print("Disconnected:", reason)
end

-- send message to server
ws:send("Hello!")

-- close connection gracefully (waits for queue)
ws:close()

-- close connection immediately
ws:close_now()

-- reopen connection (reconnect)
ws:open()
```

## api reference

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

### `tungstenite:open()`

re-opens (basically reconnect) websocket

## callbacks

### `tungstenite:on_message(string data)`

called when a text message is received from the server

### `tungstenite:on_error(string message)`

called when an error occurs

### `tungstenite:on_disconnect(string message)`

called when the connection is closed. whenever `on_disconnect` is called, socket should be considered as closed

### `tungstenite:on_connect()`

called when the connection is successfully established