use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, PhysicalPosition, Rgba8Pixel,
    SharedPixelBuffer, VecModel,
};
use std::rc::Rc;
use std::sync::Arc;

use mc_core::{
    CatalogProvider, GameVersion, GameVersionType, InMemoryTaskService, Instance, InstanceService,
    LoaderType, MockCatalogProvider, MockInstanceService, Task, TaskService, TaskStatus,
};

slint::include_modules!();

/// 从标准 64x64 Minecraft 正版皮肤 RGBA 纹理中裁剪 8x8 头部正面并合成 8x8 帽子覆盖层，放大为 64x64 像素头像
pub fn extract_head_from_skin_rgba(skin_64x64: &[u8]) -> Option<Image> {
    if skin_64x64.len() < 64 * 64 * 4 {
        return None;
    }
    let mut head_8x8 = [[0u8; 4]; 64];
    for y in 0..8usize {
        for x in 0..8usize {
            // 基础面部区域: (8..16, 8..16)
            let base_idx = ((y + 8) * 64 + (x + 8)) * 4;
            let mut r = skin_64x64[base_idx];
            let mut g = skin_64x64[base_idx + 1];
            let mut b = skin_64x64[base_idx + 2];

            // 二层帽子/刘海覆盖区域: (40..48, 8..16)
            let hat_idx = ((y + 8) * 64 + (x + 40)) * 4;
            let hr = skin_64x64[hat_idx];
            let hg = skin_64x64[hat_idx + 1];
            let hb = skin_64x64[hat_idx + 2];
            let ha = skin_64x64[hat_idx + 3] as u16;

            if ha > 0 {
                let inv_a = 255 - ha;
                r = ((hr as u16 * ha + r as u16 * inv_a) / 255) as u8;
                g = ((hg as u16 * ha + g as u16 * inv_a) / 255) as u8;
                b = ((hb as u16 * ha + b as u16 * inv_a) / 255) as u8;
            }
            head_8x8[y * 8 + x] = [r, g, b, 255];
        }
    }

    // 最近邻 8 倍放大至 64x64，确保在各 DPI 下保持锐利像素边缘
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(64, 64);
    let slice = buf.make_mut_slice();
    for py in 0..64usize {
        for px in 0..64usize {
            let [r, g, b, a] = head_8x8[(py / 8) * 8 + (px / 8)];
            slice[py * 64 + px] = Rgba8Pixel { r, g, b, a };
        }
    }
    Some(Image::from_rgba8(buf))
}

/// 生成内置正版示例皮肤纹理并走标准 64x64 皮肤头部裁剪管线
fn build_sample_genuine_skin_head() -> Image {
    let mut skin = vec![0u8; 64 * 64 * 4];
    // 填充 (8..16, 8..16) 面部底色与五官纹理
    let face_rows: [[ [u8; 3]; 8]; 8] = [
        [[46,30,18],[52,34,20],[58,38,22],[52,34,20],[46,30,18],[52,34,20],[58,38,22],[46,30,18]],
        [[46,30,18],[58,38,22],[64,42,25],[189,142,108],[189,142,108],[64,42,25],[58,38,22],[46,30,18]],
        [[46,30,18],[198,150,116],[208,160,124],[208,160,124],[208,160,124],[208,160,124],[198,150,116],[46,30,18]],
        [[198,150,116],[208,160,124],[208,160,124],[208,160,124],[208,160,124],[208,160,124],[208,160,124],[198,150,116]],
        [[198,150,116],[255,255,255],[68,56,168],[208,160,124],[208,160,124],[68,56,168],[255,255,255],[198,150,116]],
        [[189,142,108],[198,150,116],[208,160,124],[164,114,82],[164,114,82],[208,160,124],[198,150,116],[189,142,108]],
        [[180,132,98],[189,142,108],[118,68,42],[102,56,34],[102,56,34],[118,68,42],[189,142,108],[180,132,98]],
        [[172,124,90],[180,132,98],[102,56,34],[142,88,58],[142,88,58],[102,56,34],[180,132,98],[172,124,90]],
    ];
    for y in 0..8usize {
        for x in 0..8usize {
            let idx = ((y + 8) * 64 + (x + 8)) * 4;
            skin[idx] = face_rows[y][x][0];
            skin[idx + 1] = face_rows[y][x][1];
            skin[idx + 2] = face_rows[y][x][2];
            skin[idx + 3] = 255;
        }
    }
    // 填充 (40..48, 8..10) 立体刘海覆盖层
    for x in 1..7usize {
        let idx = ((1 + 8) * 64 + (x + 40)) * 4;
        skin[idx] = 68;
        skin[idx + 1] = 44;
        skin[idx + 2] = 26;
        skin[idx + 3] = 230;
    }
    extract_head_from_skin_rgba(&skin).unwrap_or_default()
}

/// 构建初始玩家账号与档案列表（正版账号使用皮肤裁剪头部，离线档案演示回退矢量头像）
fn build_player_profiles() -> Vec<PlayerProfile> {
    let genuine_avatar = build_sample_genuine_skin_head();
    vec![
        PlayerProfile {
            id: "prof-steve".into(),
            username: "Steve_AI".into(),
            uuid: "8667ba71b85a4004af54457a9734eed7".into(),
            account_type: "Microsoft 正版".into(),
            avatar: genuine_avatar,
            has_avatar: true,
            skin_tint: Color::from_rgb_u8(59, 35, 20),
            online: true,
        },
        PlayerProfile {
            id: "prof-alex".into(),
            username: "Alex_Builder".into(),
            uuid: "00000000000000000000000000000000".into(),
            account_type: "离线档案 (回退头像)".into(),
            avatar: Image::default(),
            has_avatar: false,
            skin_tint: Color::from_rgb_u8(204, 114, 46),
            online: false,
        },
    ]
}

/// 将核心层 Instance 实体转换为界面展示模型
fn map_instance_to_entry(inst: &Instance, is_active: bool) -> InstanceEntry {
    let (loader_tag, java_tag, subtitle, status, status_text, icon_type, play_time, mod_count) =
        match inst.id.as_str() {
            "inst-fabric-121" => (
                "Fabric",
                "Java 21",
                "Fabric · 1.21 · Java 21",
                if is_active { "active" } else { "installed" },
                if is_active { "当前使用" } else { "已安装" },
                "grass",
                "128 小时",
                "42 个",
            ),
            "inst-vanilla-121" => (
                "Vanilla",
                "Java 21",
                "Vanilla · 1.21 · Java 21",
                if is_active { "active" } else { "installed" },
                if is_active { "当前使用" } else { "已安装" },
                "grass",
                "36 小时",
                "0 个",
            ),
            "inst-forge-1192" => (
                "Forge",
                "Java 17",
                "Forge · 1.19.2 · Java 17",
                "uninstalled",
                "未安装",
                "anvil",
                "0 小时",
                "18 个",
            ),
            "inst-modpack-1201" => (
                "Modpack",
                "Java 17",
                "自定义整合包 · 1.20.1",
                if is_active { "active" } else { "installed" },
                if is_active { "当前使用" } else { "已安装" },
                "modpack",
                "215 小时",
                "164 个",
            ),
            _ => {
                let l_tag = inst
                    .loader
                    .as_ref()
                    .map(|l| match l.loader_type {
                        LoaderType::Fabric => "Fabric",
                        LoaderType::Forge => "Forge",
                        LoaderType::NeoForge => "NeoForge",
                        LoaderType::Quilt => "Quilt",
                        LoaderType::Vanilla => "Vanilla",
                    })
                    .unwrap_or("Vanilla");
                (
                    l_tag,
                    "Java 21",
                    "自定义实例",
                    if is_active { "active" } else { "installed" },
                    if is_active { "当前使用" } else { "已安装" },
                    "grass",
                    "0 小时",
                    "0 个",
                )
            }
        };

    InstanceEntry {
        id: inst.id.clone().into(),
        name: inst.name.clone().into(),
        version_tag: inst.mc_version.clone().into(),
        loader_tag: loader_tag.into(),
        java_tag: java_tag.into(),
        subtitle: subtitle.into(),
        status: status.into(),
        status_text: status_text.into(),
        icon_type: icon_type.into(),
        game_dir: inst
            .game_dir
            .to_string_lossy()
            .replace("\\Fabric-", "\nFabric-")
            .into(),
        play_time: play_time.into(),
        mod_count: mod_count.into(),
    }
}

/// 将核心层 Task 实体转换为界面任务队列项
fn map_task_to_entry(task: &Task) -> TaskEntry {
    let parts: Vec<&str> = task.progress.step_description.split('|').collect();
    let title = parts.first().copied().unwrap_or("后台任务");
    let progress_text = parts.get(1).copied().unwrap_or("处理中...");
    let speed_text = parts.get(2).copied().unwrap_or("0.0 MB/s");
    let icon_type = parts.get(3).copied().unwrap_or("grass");
    let status = match task.status {
        TaskStatus::Running | TaskStatus::Pending => "running",
        TaskStatus::Cancelled => "paused",
        _ => "running",
    };

    TaskEntry {
        id: task.id.clone().into(),
        title: title.into(),
        progress: (task.progress.percentage / 100.0).clamp(0.0, 1.0),
        progress_text: progress_text.into(),
        speed_text: speed_text.into(),
        status: status.into(),
        icon_type: icon_type.into(),
    }
}

/// 构建更新日志列表数据
fn build_changelog_entries() -> Vec<ChangelogEntry> {
    vec![
        ChangelogEntry {
            version: "v0.1.0".into(),
            date: "2024-12-28".into(),
            badge_kind: "latest".into(),
            line_1: "全新界面与架构设计".into(),
            line_2: "支持 AI 一键组包 (API / MCP)".into(),
            line_3: "优化中文本地化体验".into(),
        },
        ChangelogEntry {
            version: "v0.0.9".into(),
            date: "2024-12-10".into(),
            badge_kind: "normal".into(),
            line_1: "优化游戏下载速度".into(),
            line_2: "改进 Mod 管理体验".into(),
            line_3: "".into(),
        },
        ChangelogEntry {
            version: "v0.0.8".into(),
            date: "2024-11-25".into(),
            badge_kind: "warning".into(),
            line_1: "修复整合包导入异常问题".into(),
            line_2: "提升启动稳定性".into(),
            line_3: "".into(),
        },
    ]
}

/// 将核心层 CatalogProvider 返回的 GameVersion 实体映射为下载页 Slint 视图模型
fn map_game_version_to_entry(ver: &GameVersion) -> DownloadVersionEntry {
    let release_badge = match ver.version_type {
        GameVersionType::Release => "正式版",
        GameVersionType::Snapshot => "快照版",
    };
    let type_badge = if ver.version_type == GameVersionType::Snapshot {
        "快照"
    } else {
        "原版"
    };
    let get_item = |list: &[String], idx: usize| -> slint::SharedString {
        list.get(idx).map(|s| s.as_str()).unwrap_or("").into()
    };

    DownloadVersionEntry {
        id: ver.id.clone().into(),
        title: ver.title.clone().into(),
        release_badge: release_badge.into(),
        type_badge: type_badge.into(),
        release_date: ver.release_date.clone().into(),
        java_tag: ver.java_version.clone().into(),
        supported_loaders: ver.supported_loaders.join(" / ").into(),
        summary: ver.summary.clone().into(),
        changelog_url: ver.changelog_url.clone().into(),
        feature_1: get_item(&ver.highlights, 0),
        feature_2: get_item(&ver.highlights, 1),
        feature_3: get_item(&ver.highlights, 2),
        feature_4: get_item(&ver.highlights, 3),
        feature_5: get_item(&ver.highlights, 4),
        file_1: get_item(&ver.version_files, 0),
        file_2: get_item(&ver.version_files, 1),
        file_3: get_item(&ver.version_files, 2),
        file_4: get_item(&ver.version_files, 3),
        file_5: get_item(&ver.version_files, 4),
        req_1: get_item(&ver.system_requirements, 0),
        req_2: get_item(&ver.system_requirements, 1),
        req_3: get_item(&ver.system_requirements, 2),
        req_4: get_item(&ver.system_requirements, 3),
        req_5: get_item(&ver.system_requirements, 4),
        res_1: get_item(&ver.related_resources, 0),
        res_2: get_item(&ver.related_resources, 1),
        res_3: get_item(&ver.related_resources, 2),
        res_4: get_item(&ver.related_resources, 3),
        res_5: get_item(&ver.related_resources, 4),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化核心服务层：实例服务、任务队列服务与版本目录服务（当前使用 MockCatalogProvider 模拟数据）
    let instance_service = Arc::new(MockInstanceService::new().with_sample_data().await);
    let task_service = Arc::new(InMemoryTaskService::new().with_sample_data().await);
    let catalog_provider: Arc<dyn CatalogProvider> = Arc::new(MockCatalogProvider::new());

    // 实例化无边框主窗口
    let main_window = MainWindow::new()?;

    // 0. 注入玩家账号/档案列表（默认激活第一个正版皮肤头像账号）
    let initial_profiles = build_player_profiles();
    if let Some(first_prof) = initial_profiles.first() {
        main_window.set_current_profile(first_prof.clone());
    }
    let profiles_model = Rc::new(VecModel::from(initial_profiles));
    main_window.set_profiles(ModelRc::from(profiles_model.clone()));

    // 1. 从后端 InstanceService 加载实例列表并按预设顺序排序
    let order = [
        "inst-fabric-121",
        "inst-vanilla-121",
        "inst-forge-1192",
        "inst-modpack-1201",
    ];
    let mut raw_instances = instance_service.list_instances().await?;
    raw_instances.sort_by_key(|inst| {
        order
            .iter()
            .position(|&id| id == inst.id)
            .unwrap_or(usize::MAX)
    });
    let initial_entries: Vec<InstanceEntry> = raw_instances
        .iter()
        .map(|inst| map_instance_to_entry(inst, inst.id == "inst-fabric-121"))
        .collect();
    let entries_model = Rc::new(VecModel::from(initial_entries));
    main_window.set_instances(ModelRc::from(entries_model.clone()));

    // 2. 注入后端更新日志模型
    let changelog_model = Rc::new(VecModel::from(build_changelog_entries()));
    main_window.set_changelogs(ModelRc::from(changelog_model));

    // 3. 从后端 TaskService 加载任务队列模型
    let mut raw_tasks = task_service.list_tasks().await?;
    raw_tasks.sort_by(|a, b| a.id.cmp(&b.id));
    let task_entries: Vec<TaskEntry> = raw_tasks.iter().map(map_task_to_entry).collect();
    let tasks_model = Rc::new(VecModel::from(task_entries));
    main_window.set_tasks(ModelRc::from(tasks_model.clone()));

    // 4. 从后端 CatalogProvider 异步加载版本目录并注入下载视图（默认展示全部正式版，选中 1.21）
    let catalog_versions = catalog_provider.list_game_versions().await?;
    let initial_dl_versions: Vec<DownloadVersionEntry> = catalog_versions
        .iter()
        .filter(|v| v.version_type == GameVersionType::Release)
        .map(map_game_version_to_entry)
        .collect();
    if let Some(first_ver) = initial_dl_versions.first() {
        main_window.set_selected_download_version(first_ver.clone());
    }
    let dl_versions_model = Rc::new(VecModel::from(initial_dl_versions));
    main_window.set_download_versions(ModelRc::from(dl_versions_model.clone()));

    // 绑定无边框窗口内化标题栏拖拽移动事件
    {
        let window_weak = main_window.as_weak();
        main_window.on_drag_window(move |dx, dy| {
            if let Some(window) = window_weak.upgrade() {
                let win = window.window();
                let scale = win.scale_factor();
                let pos = win.position();
                let next_x = pos.x + (dx * scale) as i32;
                let next_y = pos.y + (dy * scale) as i32;
                win.set_position(PhysicalPosition::new(next_x, next_y));
            }
        });
    }

    // 绑定无边框窗口最小化、最大化/还原、关闭事件
    {
        let window_weak = main_window.as_weak();
        main_window.on_minimize_window_clicked(move || {
            if let Some(window) = window_weak.upgrade() {
                window.window().set_minimized(true);
            }
        });
    }
    {
        let window_weak = main_window.as_weak();
        main_window.on_maximize_window_clicked(move || {
            if let Some(window) = window_weak.upgrade() {
                let is_max = window.window().is_maximized();
                window.window().set_maximized(!is_max);
            }
        });
    }
    {
        let window_weak = main_window.as_weak();
        main_window.on_close_window_clicked(move || {
            if let Some(window) = window_weak.upgrade() {
                let _ = window.hide();
            }
        });
    }

    // 绑定侧边栏底部与设置页「更换账户」的玩家档案切换与新增事件
    {
        let window_weak = main_window.as_weak();
        let model = profiles_model.clone();
        main_window.on_select_profile_clicked(move |prof_id| {
            let count = model.row_count();
            for i in 0..count {
                if let Some(prof) = model.row_data(i) {
                    if prof.id == prof_id {
                        if let Some(window) = window_weak.upgrade() {
                            window.set_current_profile(prof.clone());
                            window.set_status_text(
                                format!("已切换当前玩家档案：{} ({})", prof.username, prof.account_type)
                                    .into(),
                            );
                        }
                        break;
                    }
                }
            }
        });
    }
    {
        let window_weak = main_window.as_weak();
        let model = profiles_model.clone();
        main_window.on_cycle_profile_clicked(move || {
            let count = model.row_count();
            if count == 0 {
                return;
            }
            if let Some(window) = window_weak.upgrade() {
                let curr_id = window.get_current_profile().id;
                let mut next_idx = 0usize;
                for i in 0..count {
                    if let Some(prof) = model.row_data(i) {
                        if prof.id == curr_id {
                            next_idx = (i + 1) % count;
                            break;
                        }
                    }
                }
                if let Some(next_prof) = model.row_data(next_idx) {
                    window.set_current_profile(next_prof.clone());
                    window.set_status_text(
                        format!(
                            "已同步切换当前账户：{} ({})",
                            next_prof.username, next_prof.account_type
                        )
                        .into(),
                    );
                }
            }
        });
    }
    {
        let window_weak = main_window.as_weak();
        let model = profiles_model.clone();
        main_window.on_add_profile_clicked(move || {
            let next_idx = model.row_count() + 1;
            let new_prof = PlayerProfile {
                id: format!("prof-offline-{}", next_idx).into(),
                username: format!("Player_{}", next_idx).into(),
                uuid: "00000000000000000000000000000000".into(),
                account_type: "离线档案".into(),
                avatar: Image::default(),
                has_avatar: false,
                skin_tint: Color::from_rgb_u8(42, 92, 148),
                online: false,
            };
            model.push(new_prof.clone());
            if let Some(window) = window_weak.upgrade() {
                window.set_current_profile(new_prof.clone());
                window.set_status_text(format!("已创建并切换至新档案：{}", new_prof.username).into());
            }
        });
    }

    // 绑定实例列表点击切换当前活跃实例事件
    {
        let window_weak = main_window.as_weak();
        let model = entries_model.clone();
        main_window.on_select_instance_clicked(move |selected_id| {
            if let Some(window) = window_weak.upgrade() {
                let count = model.row_count();
                for i in 0..count {
                    if let Some(mut item) = model.row_data(i) {
                        if item.id == selected_id {
                            let mut active_view = item.clone();
                            active_view.status_text = if item.status == "uninstalled" {
                                "未安装".into()
                            } else {
                                "已安装".into()
                            };
                            window.set_current_instance(active_view);
                            if item.status != "uninstalled" {
                                item.status = "active".into();
                                item.status_text = "当前使用".into();
                            }
                            window.set_status_text(format!("已切换至实例：{}", item.name).into());
                        } else if item.status == "active" {
                            item.status = "installed".into();
                            item.status_text = "已安装".into();
                        }
                        model.set_row_data(i, item);
                    }
                }
            }
        });
    }

    // 绑定实例列表拖拽排序并自动吸附事件
    {
        let window_weak = main_window.as_weak();
        let model = entries_model.clone();
        main_window.on_move_instance_clicked(move |from_idx, to_idx| {
            let from = from_idx as usize;
            let to = to_idx as usize;
            let count = model.row_count();
            if from < count && to < count && from != to {
                if let Some(moved_item) = model.row_data(from) {
                    let name = moved_item.name.to_string();
                    model.remove(from);
                    model.insert(to, moved_item);
                    if let Some(window) = window_weak.upgrade() {
                        window.set_status_text(
                            format!("实例「{}」已移动并吸附至第 {} 位", name, to + 1).into(),
                        );
                    }
                }
            }
        });
    }

    // 绑定「开始游戏」按钮事件
    {
        let window_weak = main_window.as_weak();
        main_window.on_start_game_clicked(move |instance_id| {
            if let Some(window) = window_weak.upgrade() {
                let current = window.get_current_instance();
                let prof = window.get_current_profile();
                window.set_status_text(
                    format!(
                        "正以玩家「{}」启动实例：{} ({}) ...",
                        prof.username, current.name, instance_id
                    )
                    .into(),
                );
            }
        });
    }

    // 绑定「编辑实例」按钮事件
    {
        let window_weak = main_window.as_weak();
        main_window.on_edit_instance_clicked(move |_instance_id| {
            if let Some(window) = window_weak.upgrade() {
                let current = window.get_current_instance();
                window.set_status_text(format!("已打开实例配置：{}", current.name).into());
            }
        });
    }

    // 绑定实例列表「管理 >」同步刷新事件
    {
        let window_weak = main_window.as_weak();
        let service = instance_service.clone();
        main_window.on_refresh_clicked(move || {
            let window_weak = window_weak.clone();
            let service = service.clone();
            tokio::spawn(async move {
                if let Ok(core_list) = service.list_instances().await {
                    let count = core_list.len();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = window_weak.upgrade() {
                            window.set_status_text(
                                format!("后端实例列表同步完成（共 {} 个实例）", count).into(),
                            );
                        }
                    });
                }
            });
        });
    }

    // 绑定更新日志「查看全部 >」事件
    {
        let window_weak = main_window.as_weak();
        main_window.on_view_all_changelogs_clicked(move || {
            if let Some(window) = window_weak.upgrade() {
                window.set_status_text("已加载全部历史版本更新日志".into());
            }
        });
    }

    // 绑定任务队列单项暂停/继续事件（同步调用后端 TaskService）
    {
        let window_weak = main_window.as_weak();
        let model = tasks_model.clone();
        let task_srv = task_service.clone();
        main_window.on_toggle_task_clicked(move |task_id| {
            let count = model.row_count();
            for i in 0..count {
                if let Some(mut item) = model.row_data(i) {
                    if item.id == task_id {
                        let pausing = item.status != "paused";
                        item.status = if pausing { "paused".into() } else { "running".into() };
                        let title = item.title.to_string();
                        model.set_row_data(i, item);

                        if pausing {
                            let srv = task_srv.clone();
                            let tid = task_id.to_string();
                            tokio::spawn(async move {
                                let _ = srv.cancel_task(&tid).await;
                            });
                        }

                        if let Some(window) = window_weak.upgrade() {
                            let action = if pausing { "已暂停" } else { "已继续" };
                            window.set_status_text(format!("任务「{}」{}", title, action).into());
                        }
                        break;
                    }
                }
            }
        });
    }

    // 绑定任务队列「全部暂停 >」事件
    {
        let window_weak = main_window.as_weak();
        let model = tasks_model.clone();
        main_window.on_pause_all_tasks_clicked(move || {
            let count = model.row_count();
            let mut any_running = false;
            for i in 0..count {
                if let Some(item) = model.row_data(i) {
                    if item.status != "paused" {
                        any_running = true;
                        break;
                    }
                }
            }
            let target_status = if any_running { "paused" } else { "running" };
            for i in 0..count {
                if let Some(mut item) = model.row_data(i) {
                    item.status = target_status.into();
                    model.set_row_data(i, item);
                }
            }
            if let Some(window) = window_weak.upgrade() {
                window.set_status_text(
                    if any_running {
                        "已暂停队列中全部后台任务".into()
                    } else {
                        "已恢复队列中全部后台任务".into()
                    },
                );
            }
        });
    }

    // 绑定游戏下载页面版本选中、条件筛选检索与一键安装事件（全部调用后端 CatalogProvider 与 TaskService）
    {
        let window_weak = main_window.as_weak();
        let catalog = catalog_provider.clone();
        main_window.on_select_download_version_clicked(move |ver_id| {
            let window_weak = window_weak.clone();
            let catalog = catalog.clone();
            let id_str = ver_id.to_string();
            tokio::spawn(async move {
                if let Ok(Some(ver)) = catalog.get_game_version(&id_str).await {
                    let entry = map_game_version_to_entry(&ver);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = window_weak.upgrade() {
                            window.set_selected_download_version(entry.clone());
                            window.set_status_text(
                                format!(
                                    "已加载版本详情：{} ({} · {})",
                                    entry.title, entry.java_tag, entry.supported_loaders
                                )
                                .into(),
                            );
                        }
                    });
                }
            });
        });
    }
    {
        let window_weak = main_window.as_weak();
        let catalog = catalog_provider.clone();
        main_window.on_filter_download_versions_clicked(
            move |query, scope_idx, type_idx, loader_idx, sort_idx| {
                let window_weak = window_weak.clone();
                let catalog = catalog.clone();
                let q = query.trim().to_lowercase();
                tokio::spawn(async move {
                    if let Ok(mut list) = catalog.list_game_versions().await {
                        // 1. 范围与版本类型筛选 (默认仅展示正式版，选择快照筛选时展示快照版)
                        list.retain(|v| {
                            let match_scope = match scope_idx {
                                1 => v.version_type == GameVersionType::Release,
                                2 => v.version_type == GameVersionType::Snapshot,
                                _ => true,
                            };
                            let match_type = match type_idx {
                                1 => v.version_type == GameVersionType::Release,
                                2 => v.version_type == GameVersionType::Snapshot,
                                _ => true,
                            };
                            let match_loader = match loader_idx {
                                1 => v.supported_loaders.iter().any(|l| l.contains("Fabric")),
                                2 => v
                                    .supported_loaders
                                    .iter()
                                    .any(|l| l.contains("Forge") || l.contains("NeoForge")),
                                _ => true,
                            };
                            let match_query = if q.is_empty() {
                                true
                            } else {
                                v.id.to_lowercase().contains(&q)
                                    || v.title.to_lowercase().contains(&q)
                                    || v.summary.to_lowercase().contains(&q)
                                    || v.java_version.to_lowercase().contains(&q)
                            };
                            match_scope && match_type && match_loader && match_query
                        });

                        // 2. 排序方式
                        if sort_idx == 1 {
                            list.sort_by(|a, b| a.release_time.cmp(&b.release_time));
                        } else {
                            list.sort_by(|a, b| b.release_time.cmp(&a.release_time));
                        }

                        let entries: Vec<DownloadVersionEntry> =
                            list.iter().map(map_game_version_to_entry).collect();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(window) = window_weak.upgrade() {
                                let count = entries.len();
                                if let Some(first) = entries.first() {
                                    window.set_selected_download_version(first.clone());
                                }
                                let new_model = Rc::new(VecModel::from(entries));
                                window.set_download_versions(ModelRc::from(new_model));
                                window.set_status_text(
                                    format!("已从版本目录检索到 {} 个匹配版本", count).into(),
                                );
                            }
                        });
                    }
                });
            },
        );
    }
    {
        let window_weak = main_window.as_weak();
        let tasks = tasks_model.clone();
        main_window.on_install_version_clicked(move |ver_id| {
            let task_id = format!("task-dl-{}", ver_id);
            let title = format!("Minecraft {}", ver_id);
            let mut exists = false;
            for i in 0..tasks.row_count() {
                if let Some(mut t) = tasks.row_data(i) {
                    if t.id == task_id.as_str() || t.title == title.as_str() {
                        t.status = "running".into();
                        tasks.set_row_data(i, t);
                        exists = true;
                        break;
                    }
                }
            }
            if !exists {
                tasks.insert(
                    0,
                    TaskEntry {
                        id: task_id.into(),
                        title: title.clone().into(),
                        progress: 0.12,
                        progress_text: "64 MB / 520 MB (12%)".into(),
                        speed_text: "14.8 MB/s".into(),
                        status: "running".into(),
                        icon_type: "grass".into(),
                    },
                );
            }
            if let Some(window) = window_weak.upgrade() {
                window.set_status_text(
                    format!("已将「{}」加入下载任务队列，可在主页任务队列查看进度", title).into(),
                );
            }
        });
    }

    // 启动原生主事件循环
    main_window.run()?;

    Ok(())
}
