require("tungstenite")

local conn = tungstenite.connect("wss://echo.websocket.org")

function conn:on_connect()
  print("connected to websocket")
end
function conn:on_message(message)
  -- before echo'd message, there will be some system message from server
  print("received", message)
end
function conn:on_error(err)
  print("error:", err)
end
function conn:on_disconnect(reason)
  print("disconnected (", reason, ")")
end

print(conn) -- "tungstenite (uuidv4)"

for i = 1, 10 do
  conn:send(("hello, gmsv_tungstenite! (%d)"):format(i))
end