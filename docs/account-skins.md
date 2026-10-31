# 账户头像与离线皮肤

侧栏和账户管理共用同一张皮肤头像，裁剪皮肤的头部正面并叠加帽子层，使用最近邻显示。兼容 64×64 和 64×32 PNG；旧版 RGB 皮肤的空白不透明帽子区域按原版规则处理。

Microsoft 账户从 Mojang 玩家档案读取纹理地址，再从 `textures.minecraft.net` 下载皮肤。成功后缓存到启动器数据目录的 `cache/account-skins/<UUID>.png`；重启时先显示缓存，再刷新。网络失败时保留缓存，显示失败信息，可点击「刷新皮肤」重试；重新打开账户管理也会重试失败的请求。

离线账户无需联网，按 UUID 的 Java hashCode 自动选择原版默认皮肤。账户行提供「设置皮肤」和「恢复默认皮肤」；选择的 PNG 保存到 `account-skins/<UUID>.png`，重启后继续使用。自定义离线皮肤与可清理的在线缓存分开保存。新选择使用请求代次隔离，不会被较早的头像加载结果覆盖。

游戏内离线皮肤沿用本地资源包方式，目前支持 Minecraft 1.20.1 / 1.21.1，要求 64×64 PNG。它会替换该 UUID 对应的原版默认皮肤，相同默认皮肤的其他玩家也会受到影响。64×32 PNG 可用作启动器头像。恢复默认皮肤后，下次启动会停用启动器的离线皮肤资源包。

验证记录：桌面端 118 项测试通过、21 项按条件跳过；另外执行了 Mojang 真实皮肤下载与两种窗口尺寸的四张头像截图。Windows 原生账户窗口截图位于 `target/account-skin/screenshots/`。本次没有启动 Minecraft，游戏内显示仍需实机检查。

原生截图入口：`cargo run --locked --offline -p desktop -- --accounts --screenshot target/account-skin/screenshots/accounts.png --size 1024x680`。验证时应使用独立的 `MEMORIA_DATA_DIR`。
