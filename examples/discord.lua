require("tungstenite")

local discord = {}
discord.connection = discord.connection or tungstenite.connect("wss://gateway.discord.gg/?v=10&encoding=json")

local connection = discord.connection
function connection:on_message(message)
  local response = util.JSONToTable(message)
  if response == nil then return end

  if response.op == 10 then
    local interval = response.d.heartbeat_interval / 1000
    create_heartbeat(interval)

    local identify = {
      op = 2,
      d = {
        token = "x",
        intents = bit.lshift(1, 0) + bit.lshift(1, 9) + bit.lshift(1, 15), -- @note: GUILDS, GUILD_MESSAGES, MESSAGE_CONTENT
        properties = {
          os = "linux",
          browser = "gmod/tungstenite",
          device = "pc"
        },
        presence = {
          activites = {
            {
              name = "github.com/shockpast",
              type = 0
            }
          }
        }
      }
    }
    
    -- heartbeat()
    connection:send(util.TableToJSON(identify))
  end

  if response.op == 1 then
    heartbeat()
  end

  if response.op == 11 then
    print("[tungstenite/discord] hearbeat acknowledged")
  end

  if response.op == 0 and response.t == "READY" then
    print("[tungstenite/discord] handshake completed")
    print(("[tungstenite/discord] logged in as %s"):format(response.d.user.username))
  end
  if response.op == 0 and response.t == "MESSAGE_CREATE" then
    print(("[tungstenite/discord] %s: %s"):format(response.d.author.username, response.d.content))
  end
end
function connection:on_error(err)
  print(("[tungstenite/discord] error: %s", err))
end

function heartbeat()
  -- d will be omitted by TableToJSON, but this field might be useful in future
  -- read https://discord.com/developers/docs/events/gateway#connections for more
  connection:send(util.TableToJSON({ op = 1, d = nil }))
  print("[tungstenite/discord] sent heartbeat")
end

function create_heartbeat(interval)
  timer.Create("discord.heartbeat", interval, 0, heartbeat)
  print(("[tungstenite/discord] sending heartbeat every %d seconds"):format(interval))
end
