# 工作区界面截图

由实际 iced 控件树在 Linux tiny-skia 后端渲染，使用 Noto Sans CJK SC 字体和测试实例。账户、实例安装状态、最近打开时间及对话属于截图 fixture，不代表用户本机资料或已经执行过游戏启动。

- [对话开屏](chat.png)
- [主页与实例统计](home.png)
- [加号与上下文菜单](context.png)
- [MCP 服务、权限和实例授权](mcp.png)

测试入口为 `capture_workspace_redesign` 和 `capture_context_and_mcp_settings`，均为显式运行的 ignored 测试；使用 `MEMORIA_UI_CAPTURE_DIR` 指定新的输出目录。截图不等同于 Windows GPU、Microsoft 登录或游戏启动验证。
