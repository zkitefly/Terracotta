# 陶瓦内部协议与组网

> 本文档基于版本 `v0.4.2` 源码整理。

本文档描述陶瓦联机的内部协议与组网方式。

陶瓦的联机分为两层：

1. **EasyTier 组网层**：基于 EasyTier 构建 P2P 虚拟网络，房主与房客通过网络名/密钥加入同一网络，利用端口转发暴露服务。
2. **脚手架协议层**：房主在虚拟网络内运行一个脚手架 TCP 服务，房客通过该服务完成握手、获取 MC 端口、心跳、同步玩家列表。

## 一、邀请码与网络标识

房主创建房间后会得到形如 `U/XXXX-XXXX-XXXX-XXXX` 的邀请码（源码中对应 `Room` 结构体的 `code` 字段，即 `room.code`）。邀请码中编码了 EasyTier 网络所需的 `network_name` 与 `network_secret`，房客仅需邀请码即可加入同一网络。

### 字符集

邀请码使用 34 进制字符集（不含易混淆的 `I` 和 `O`）：

```
0123456789ABCDEFGHJKLMNPQRSTUVWXYZ
```

解析时 `I` 会被纠正为 `1`，`O` 会被纠正为 `0`，且大小写不敏感。

### 邀请码格式

邀请码以固定前缀 `U/` 开头，后面紧跟 16 个 34 进制字符，并按每 4 个一组用短横线 `-` 分隔，整体形如 `U/XXXX-XXXX-XXXX-XXXX`。这 16 个字符可以解码为一个 `u128` 整数 `value`，为了防止误输入，`value` 必须能被 7 整除，否则视为无效。

### 编码：从 `value` 生成邀请码与网络标识

生成时，从满足被 7 整除条件的 `value` 出发，连续对 34 取余 16 次，依次得到从低位到高位的 16 个字符 `v0` 到 `v15`（每一步先取 `value % 34` 得到当前字符，再将 `value` 除以 34 继续下一位）。这 16 个字符按位置拼接：邀请码是 `U/` 加上完整的 16 个字符并每 4 个插入一个短横线；`network_name` 是 `scaffolding-mc-` 加上低 8 位 `v0` 到 `v7`（同样每 4 个字符插入短横线）；`network_secret` 则取高 8 位 `v8` 到 `v15`，每 4 个字符插入短横线。

### 解码：从邀请码恢复 `value`

解析时，先将输入转为大写，并把其中的 `I` 视作 `1`、`O` 视作 `0`。随后在整段文本中滑动查找以 `U/` 开头、长度为 19 个字符的片段（即 `U/` 加上 `XXXX-XXXX-XXXX-XXXX`）。从该片段的最后一个字符 `v15` 开始向前逐位计算 `value`：每遇到一个非短横线字符，就执行 `value = value * 34 + 该字符对应的数值`，而位置 4、9、14 处必须是短横线。最终校验 `value` 是否能被 7 整除，不满足则该邀请码无效。这种滑动查找的方式意味着邀请码可以嵌在任意更长的文本中，而不要求它独占整段输入。

## 二、EasyTier 组网

陶瓦使用的 EasyTier 版本为 `v2.5.0-terracotta.2`。以下命令行参数适用于 `easytier-core` 可执行文件；也可使用等价的 EasyTier SDK / API 配置。

### 通用启动参数

房主与房客均需以下参数：

```bash
easytier-core \
  --network-name "<network_name>" \
  --network-secret "<network_secret>" \
  -p "<节点>" \
  --no-tun \
  --compression=zstd \
  --multi-thread \
  --latency-first \
  --enable-kcp-proxy \
  -l "udp://0.0.0.0:0" \
  -l "tcp://0.0.0.0:0" \
  --p2p-only \
  -r <rpc_port>
```

| 参数 | 说明 |
| --- | --- |
| `--network-name` | 由邀请码派生的网络名 |
| `--network-secret` | 由邀请码派生的网络密钥 |
| `-p` | EasyTier 公共节点，可重复指定 |
| `--no-tun` | 不创建虚拟网卡 |
| `--compression=zstd` | 使用 zstd 压缩 |
| `--multi-thread` | 多线程模式 |
| `--latency-first` | 优先低延迟路由 |
| `--enable-kcp-proxy` | 启用 KCP 代理，提升弱网连通性 |
| `-l udp://0.0.0.0:0` / `-l tcp://0.0.0.0:0` | 监听随机端口（UDP + TCP） |
| `--p2p-only` | 仅使用 P2P 连接，流量不经过公共节点 |
| `-r <rpc_port>` | EasyTier RPC 控制端口，本地随机可用端口 |

### 内置公共节点

陶瓦默认追加以下公共节点：

```
https://etnode.zkitefly.eu.org/node1
https://etnode.zkitefly.eu.org/node2
```

集成方可自行追加更多节点（对应陶瓦 HTTP API 的 `public_nodes` 参数）。

### 房主额外参数

房主还需指定：

```bash
  --hostname "scaffolding-mc-server-<scaffolding_port>" \
  --ipv4 "10.144.144.1" \
  --tcp-whitelist=<scaffolding_port> \
  --tcp-whitelist=<mc_port> \
  --udp-whitelist=<mc_port>
```

- `--hostname`：房主在虚拟网络中的主机名，格式固定为 `scaffolding-mc-server-` 加上脚手架服务端口。房客通过此前缀识别房主并解析脚手架端口。
- `--ipv4 10.144.144.1`：房主在虚拟网络中的固定 IP。
- `--tcp-whitelist` / `--udp-whitelist`：白名单端口，仅允许虚拟网络内访问脚手架端口与 Minecraft 端口。

### 房客额外参数

房客使用 DHCP 自动获取虚拟网络 IP，并将 TCP / UDP 白名单端口设为 `0`：

```bash
  -d \
  --tcp-whitelist=0 \
  --udp-whitelist=0
```

- `--tcp-whitelist=0` / `--udp-whitelist=0`: 拒绝所有入站连接——房客只需通过端口转发主动连接房主，无需向虚拟网络暴露任何端口。确保房客本地网络不被外部访问。

### 节点发现

启动后，通过 `easytier-cli` 查询网络中的节点：

```bash
easytier-cli -p "127.0.0.1:<rpc_port>" -o json peer
```

返回 JSON 数组，每个节点包含：

| 字段 | 说明 |
| --- | --- |
| `hostname` | 节点主机名 |
| `ipv4` | 节点在虚拟网络中的 IPv4 地址 |
| `cost` | 路由开销，`"Local"` 表示本机 |
| `nat_type` | NAT 类型 |

房客需要：

1. **识别本机**：`cost == "Local"` 的节点即本机，用于获取本地 NAT 类型。
2. **识别房主**：`hostname` 以 `scaffolding-mc-server-` 开头的节点即房主：
   - 房主虚拟 IP = 该节点的 `ipv4`
   - 脚手架端口 = `hostname` 去掉 `scaffolding-mc-server-` 前缀后的数字部分

### 端口转发

房客通过 `easytier-cli` 添加端口转发规则，将房主的服务映射到本地：

```bash
# 脚手架端口转发（TCP）
easytier-cli -p "127.0.0.1:<rpc_port>" port-forward add tcp \
  "0.0.0.0:<local_scaffolding_port>" "<host_ip>:<scaffolding_port>"

# Minecraft 端口转发（TCP + UDP，建议同时绑定 IPv4 与 IPv6）
easytier-cli -p "127.0.0.1:<rpc_port>" port-forward add tcp \
  "0.0.0.0:<local_mc_port>" "<host_ip>:<mc_port>"
easytier-cli -p "127.0.0.1:<rpc_port>" port-forward add udp \
  "0.0.0.0:<local_mc_port>" "<host_ip>:<mc_port>"
```

- 陶瓦会优先尝试让 `<local_mc_port>` 与房主的 `<mc_port>` 相同，以保证 SimpleVoiceChat 等依赖 UDP 的 Mod 正常工作；失败则退化为随机端口。
- 脚手架端口转发只需 TCP。

### NAT 连接难度（可选，用于显示）

根据双方 NAT 类型估算连接难度，用于向用户提示：

```
if 任一方 == OpenInternet      → Easiest
else if 任一方 in [NoPAT, FullCone]    → Simple
else if 任一方 in [Restricted, PortRestricted] → Medium
else                                    → Tough
```

NAT 类型取值：`Unknown`、`OpenInternet`、`NoPAT`、`FullCone`、`Restricted`、`PortRestricted`、`Symmetric`、`SymmetricUdpWall`、`SymmetricEasyIncrease`、`SymmetricEasyDecrease`。

## 三、脚手架协议

房主在虚拟网络内运行脚手架 TCP 服务（默认端口 `13448`，失败回退随机端口）。房客通过上文的端口转发连接到本地端口，使用本协议完成接入。

### 传输层

- **协议**：TCP（IPv4 / IPv6）
- **超时**：读写超时均为 64 秒
- **连接模型**：每个房客与房主维护一条长连接，串行发送请求 / 接收响应

### 帧格式

#### 请求帧（客户端 → 服务端）

```
+---------------+------------------------+------------------------+
| kind_size(1B) | kind string (UTF-8)    | body_size(4B, big-endian) |
+---------------+------------------------+------------------------+
|                                     body (body_size bytes)                  |
+-----------------------------------------------------------------------------+
```

| 字段 | 长度 | 说明 |
| --- | --- | --- |
| `kind_size` | 1 字节 | `kind` 字符串的字节长度（最大 255） |
| `kind` | `kind_size` 字节 | UTF-8 字符串，格式为 `namespace:path`，如 `c:ping` |
| `body_size` | 4 字节 | `body` 的字节长度，大端无符号整数 |
| `body` | `body_size` 字节 | 请求体，结构由具体协议定义 |

#### 响应帧（服务端 → 客户端）

```
+------------+------------------------+----------------------------+
| status(1B) | body_size(4B, big-endian) | body (body_size bytes) |
+------------+------------------------+----------------------------+
```

| 字段 | 长度 | 说明 |
| --- | --- | --- |
| `status` | 1 字节 | 状态码：`0` 表示成功，非 `0` 表示失败 |
| `body_size` | 4 字节 | `body` 的字节长度，大端无符号整数 |
| `body` | `body_size` 字节 | 响应体；失败时通常为错误描述文本 |

未实现的协议返回 `status = 255`，body 为 `Requested protocol hasn't been implemented.`；处理异常同样返回 `status = 255`。

### 协议列表

当前所有协议位于 `c` 命名空间下：

| 路径 | 功能 |
| --- | --- |
| `c:ping` | 握手 / 指纹验证 |
| `c:protocols` | 列出服务端支持的全部协议 |
| `c:server_port` | 获取房主 Minecraft 服务器端口 |
| `c:player_ping` | 房客心跳与玩家注册 |
| `c:player_profiles_list` | 获取房间内全部玩家列表 |

---

### `c:ping` — 握手 / 指纹验证

验证连接的对端确实是陶瓦脚手架服务。

- **请求 body**：固定 16 字节指纹：

  ```
  41 57 48 44 86 37 40 59 57 44 92 43 96 99 85 01
  ```

- **响应 body**：原样回显请求 body。
- **校验**：比对响应是否等于指纹；不一致则放弃连接。

### `c:protocols` — 列出支持的协议

- **请求 body**：空。
- **响应 body**：以 `\0`（NUL 字节）分隔的 `namespace:path` 列表。

  当前返回：

  ```
  c:ping\0c:protocols\0c:server_port\0c:player_ping\0c:player_profiles_list
  ```

### `c:server_port` — 获取房主 Minecraft 服务器端口

- **请求 body**：空。
- **响应 body**（成功）：2 字节大端无符号整数，即房主 Minecraft 服务器监听的端口。
- **失败**：`status = 32`，房主不在 `HostOk` 状态。

### `c:player_ping` — 房客心跳与玩家注册

房客定期（每 5 秒）调用，上报自身信息并刷新心跳。

- **请求 body**：JSON：

  ```json
  {
    "name": "玩家名称",
    "machine_id": "32 字符 hex 机器标识",
    "vendor": "Terracotta 0.4.2, EasyTier v2.5.0-terracotta.2"
  }
  ```

- **响应 body**：空。

服务端行为：

- `machine_id` 已存在且非房主：更新心跳时间；若 `name` 变化则同步更新。
- `machine_id` 是房主：返回错误。
- `machine_id` 不存在：新增 `kind = GUEST` 的玩家。

超过 10 秒未心跳的房客会被自动移出列表。

### `c:player_profiles_list` — 获取房间内全部玩家

- **请求 body**：空。
- **响应 body**：JSON 数组：

  ```json
  [
    {
      "name": "玩家名称",
      "machine_id": "32 字符 hex 机器标识",
      "vendor": "Terracotta 0.4.2, EasyTier v2.5.0-terracotta.2",
      "kind": "HOST"
    }
  ]
  ```

| `kind` | 含义 |
| --- | --- |
| `HOST` | 房主 |
| `GUEST` | 房客 |

房主自身以 `HOST` 出现；房主本机的 `LOCAL` 玩家不会出现在此列表。

## 四、关键数据

### MACHINE_ID

- 16 字节随机数的 hex 编码（32 个十六进制字符）。
- 首次启动时生成并持久化，之后保持不变。
- 用于唯一标识设备，避免玩家重复计数。

### VENDOR

- 格式：`Terracotta {terracotta_version}, EasyTier {easytier_version}`，如 `Terracotta 0.4.2, EasyTier v2.5.0-terracotta.2`。
- 用于标识客户端来源与版本。

## 五、完整接入流程（房客视角）

1. **解析邀请码**：从邀请码派生 `network_name` 与 `network_secret`。
2. **启动 EasyTier**：使用通用参数 + `-d`（DHCP）加入网络。
3. **发现房主**：通过 `easytier-cli peer` 找到 `hostname` 以 `scaffolding-mc-server-` 开头的节点，获取房主虚拟 IP 与脚手架端口。
4. **转发脚手架端口**：将房主脚手架端口转发到本地，连接 `127.0.0.1:<local_scaffolding_port>`。
5. **握手验证**：发送 `c:ping`（携带指纹），校验响应。
6. **获取 MC 端口**：发送 `c:server_port`，读取 2 字节大端端口号。
7. **转发 MC 端口**：将房主 MC 端口转发到本地（TCP + UDP），并在本地启动一个假的 Minecraft 服务器作为「陶瓦联机大厅」入口（显示 MOTD `§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）`）。
8. **心跳与列表同步**：每 5 秒发送 `c:player_ping` 与 `c:player_profiles_list`。
