# MCP 实例写入与 FTB 任务格式

在 **设置 → AI 设置 → MCP 服务器** 启用相应类别，勾选实例并保存。四类写权限默认关闭，互相独立：来源 ID 安装 Mod、直接上传 Mod JAR、配置文件、FTB 任务。授权是允许外部客户端在所选实例执行该类别操作；保存后客户端可以生成并应用计划。关闭类别或移除实例会立即阻止后续调用，包括先前预览的计划。

未保存 `mcp.json` 的独立 CLI 继续只提供原来的六个只读工具。客户端连接配置由设置页复制；CurseForge Key 可在外部客户端的 `env` 中安全注入 `CURSEFORGE_API_KEY`，不要放在对话文本、任务文件或普通配置里。

## 预览，再应用

所有 `minecraft_write_*`、`minecraft_upsert_quest_entry` 和 `minecraft_insert_quest_image` 只生成暂存计划，返回 `plan_id`、`plan_hash`、文件路径、旧/新 SHA-256、文本预览、实际坐标和检查信息。预览文本最多 8192 个字符；文件完整内容由哈希约束。

核对结果后调用：

```json
{
  "name": "minecraft_apply_instance_write",
  "arguments": {
    "instance_id": "versions/my-pack",
    "plan_id": "<预览返回的 UUID>",
    "plan_hash": "<预览返回的 SHA-256>"
  }
}
```

计划有效期 30 分钟；每个实例最多保留 16 个待应用计划、256 MiB 待应用文件，准备新计划时清理过期且未中断的预览。应用前再次检查当前授权、实例目录身份、目标文件及读取依赖的哈希；变化返回 `PLAN_STALE`，应重新预览。不能传 `approved=true` 或自行扩大路径范围。

覆盖前备份到实例内 `.launcher/mcp/backups/<plan_id>/`，成功回执同时写入该目录的 `receipt.json`。每个文件通过同目录临时文件重命名替换；普通失败会尝试回滚已替换的文件。多文件写入不是跨文件的断电原子事务：进程中断后返回 `RECOVERY_REQUIRED`（过期后须重新预览），需要检查 `applying.json`、目标哈希和备份进行恢复。锁只协调 Memoria 写入；编辑前建议退出游戏，避免游戏或其他编辑器覆盖配置。应用成功后相同计划可重复查询；若已应用文件又变化则拒绝。

## 工具与范围

| 工具 | 入参要点 | 检查与范围 |
| --- | --- | --- |
| `minecraft_get_write_formats` | 空对象 | 格式、限制和示例，不读实例 |
| `minecraft_write_mod_list` | `instance_id`, `mods:[{source,project_id,file_id?}]` | `source` 为 `modrinth` / `curseforge`；解析实例版本和 Loader，固定真实文件，验证大小、SHA-1 和 JAR 元信息；写 `mods/*.jar` 与 `.launcher/mod-list.json` |
| `minecraft_write_mod_file` | `instance_id`, `filename`, `data_base64`, `sha256` | 真实 JAR，单个 ≤32 MiB；路径和元信息校验；直接上传不推断兼容性 |
| `minecraft_write_config` | `instance_id`, `path`, `content` | ≤1 MiB UTF-8；只允许 `config/`、`defaultconfigs/` 的 json/toml/snbt/cfg/ini/properties/yaml/yml/txt；JSON/TOML/SNBT 检查语法，其余仅检查路径和文本 |
| `minecraft_upsert_quest_entry` | 下方语义化格式 | 写 FTB 章节；检查物品、上级引用、循环、对象 ID 和数量 |
| `minecraft_insert_quest_image` | 章节、文件名、真实 Base64 图片和坐标 | PNG/JPEG ≤4 MiB、4096×4096，检查实际解码与扩展名；写图片与章节 |
| `minecraft_read_quest_entries` | `instance_id` | 读取现有章节、原生 ID、条件、奖励、依赖与图片 |
| `minecraft_validate_item_ids` | `instance_id`, `item_ids:[...]` | 返回每个 ID 的真实校验状态；需任务类别权限 |
| `minecraft_apply_instance_write` | `instance_id`, `plan_id`, `plan_hash` | 再次检查计划实际类别，而非只要有任意一项写权限即可 |

所有实例工具均受白名单限制，拒绝绝对路径、路径穿越、符号链接、Windows 重解析点与设备名。配置权限不能写 `config/ftbquests/` 或 `defaultconfigs/ftbquests/`，也不能写启动器元数据、启动脚本或存档。没有任意命令、删除、启动游戏或修改其他实例的接口。

来源安装只处理明确列出的 Mod，不自动求解/安装依赖，也不删除旧 JAR。若已有来源记录指向另一个仍存在的文件名，拒绝重复安装并要求先由用户检查旧文件。无法识别没有来源记录的同一 Mod 的旧文件，安装前仍须检查现有列表。CurseForge 必须允许第三方分发并提供 `downloadUrl`；不构造地址绕过限制。源站不可用、无匹配版本、缺 Key 或哈希错误都返回实际错误。当前网络环境下使用回环 HTTP 测试了两种源的协议与拒绝分支，尚未完成带真实 Key 的 CurseForge 在线安装验证。

## AI 任务格式

[可复制的完整示例](examples/ftb-quest-entry.json)，也可在 MCP 设置中点击“复制任务示例”。适配 **FTB Quests 1.20.1 SNBT**，不支持最新 JSON5；发现 JSON5 章节时拒绝继续。

```json
{
  "instance_id": "versions/my-pack",
  "chapter": "getting_started",
  "chapter_title": "起步",
  "entry": {
    "id": "gather_logs",
    "parent": null,
    "title": "收集木头",
    "description": ["获得 16 个橡木原木。"],
    "icon": "minecraft:oak_log",
    "completion": [{"type":"item","item":"minecraft:oak_log","count":16}],
    "rewards": [{"type":"item","item":"minecraft:apple","count":4}],
    "position": {"x":0,"y":0}
  }
}
```

`chapter` 是章节文件名（不含扩展名），创建章节需 `chapter_title`。`id` 是全书稳定的友好键，或现有条目的 16 位十六进制 ID；友好键确定性映射为原生 ID，重复调用更新同一条目。`parent` 是前置任务 ID，映射为 FTB `dependencies`；支持引用其他章节，拒绝不存在的上级和循环。

`description` 可传一段字符串或逐行字符串数组。`completion` 为物品检测（数量 1–1000000）或 `{"type":"checkmark"}`。`rewards` 目前仅支持物品奖励（数量 1–8192），禁止命令奖励。`icon` 直接传物品 ID，接口生成 FTB 的物品图标表达式。更新按请求整体替换标题、说明、上级、图标、坐标、条件和奖励；其他章节、未指定条目与未知条目字段保留，SNBT 重排版会移除注释。不会写玩家完成进度；替换条件/奖励可能改变其对象 ID，应在正式存档使用前检查进度影响。

坐标使用任务画布的逻辑单位，中心 `(0,0)`，向右 x 为正、向下 y 为正；缺省任务位置在中心。API 细网格为 **0.1**，自动吸附并返回实际坐标。例如 `x:0.14` 返回 `x:0.1`。不是屏幕像素，且不会修改玩家视口；旧版 FTB 的 Shift 行为为自由拖动，不能假定所有版本原生 Shift 网格均为 0.1。

图片通过独立工具插入：

```json
{
  "instance_id":"versions/my-pack",
  "chapter":"getting_started",
  "filename":"guide.png",
  "data_base64":"<真实 PNG 文件的 Base64>",
  "position":{"x":0,"y":-1.5},
  "width":2,
  "height":1
}
```

必须提供实际文件和 `position`；宽高同为任务单位，缺省各为 1。相同章节和文件 URI 再次插入会更新图片。图片保存在实例内，以 FTB 支持的 `file:` URI 引用；路径与本机实例绑定，移动实例后须重新插入，不宣称跨机器整合包可移植。

## 物品 ID 的真实性检查

不能根据贴图、语言文件或物品模型文件猜测某个物品一定注册：方块、动态物品和共享模型都可能造成误判。

1. 从**该实例相同 Minecraft、Loader 和 Mod 组合的实际运行环境或数据生成器**导出物品注册表。
2. 在 MCP 设置中选中实例，点击“导入物品表”。支持 Mojang `reports/registries.json` 的 `"minecraft:item":{"entries":{"namespace:item":{...}}}`，或从真实注册表转换的 `{"items":["minecraft:oak_log","example:gear"]}`；不是 Modrinth 项目列表。最多 200000 项、16 MiB。启动器信任用户选择的导出文件，不把它宣称为实时游戏探测；请勿导入手工猜测清单。
3. 导入记录绑定实例版本 JSON 与所有 Mod JAR 的 SHA-256。增加、移除、更新或禁用 JAR 后应重新导出、导入；仅改配置导致动态注册变化也需要用户主动重新导入，当前不会检测全部运行期条件。
4. AI 先调用 `minecraft_validate_item_ids`。检查命名格式、安装 JAR 的模组 ID（Fabric/Quilt/Forge/NeoForge/旧版元信息，含声明的嵌套 JAR）、注册表新鲜度和物品是否实际存在。模组使用与自身 ID 不同的物品命名空间时可能被保守拒绝。
5. `INVALID_ITEM_ID`、`MOD_NOT_INSTALLED`、`ITEM_REGISTRY_REQUIRED`、`ITEM_REGISTRY_STALE`、`ITEM_NOT_FOUND` 均会阻止涉及该物品的任务计划。图标、条件和奖励遵循同一规则，绝不把“Mod 存在”当成“物品存在”。

MCP 没有导入受信任注册表的工具，模型不能靠传入任意 `items` 绕过存在性检查。自动勾选条件且没有物品图标/奖励的任务可不依赖注册表。

## 格式依据与验证范围

对照 FTB 上游 `1.20.1/main` 分支的 [BaseQuestFile](https://github.com/FTBTeam/FTB-Quests/blob/1.20.1/main/common/src/main/java/dev/ftb/mods/ftbquests/quest/BaseQuestFile.java)、[ItemTask](https://github.com/FTBTeam/FTB-Quests/blob/1.20.1/main/common/src/main/java/dev/ftb/mods/ftbquests/quest/task/ItemTask.java)、[ChapterImage](https://github.com/FTBTeam/FTB-Quests/blob/1.20.1/main/common/src/main/java/dev/ftb/mods/ftbquests/quest/ChapterImage.java) 与 FTB Library Icon 的读取行为实现。自动测试验证生成文件读回、哈希、授权、路径、图片解码、来源协议和 stdio 完整调用。当前没有启动实际 FTB 游戏客户端进行视觉或完成进度验证。
