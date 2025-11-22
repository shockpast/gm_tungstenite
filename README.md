# 🪨 gmsv_tungstenite

websocket client for garry's mod servers, inspired by [FredyH/GWSockets](https://github.com/FredyH/GWSockets)

## example

```lua
require("tungstenite")

local conn = tungstenite.connect("wss://echo.websocket.org")

function conn:on_connect()
  print("connected to websocket")
end
function conn:on_message(message)
  print("received", message)
end
function conn:on_error(err)
  print("error:", err)
end
function conn:on_disconnect(reason)
  print("disconnected (", reason, ")")
end

print(conn) -- "tungstenite (uuidv4)"

conn:send("hello world!")
```