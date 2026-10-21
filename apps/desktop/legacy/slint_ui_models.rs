use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};

use crate::{
    AiPromptEntry, AiShowcaseEntry, AiTaskEntry, ChangelogEntry, DownloadVersionEntry,
    InstanceEntry, PlayerProfile, TaskEntry,
};
use mc_core::{GameVersion, GameVersionType, Instance, LoaderType, Task, TaskStatus};

/// 从标准 64x64 Minecraft 正版皮肤 RGBA 纹理中裁剪 8x8 头部正面并合成 8x8 帽子覆盖层，放大为 64x64 像素头像
pub fn extract_head_from_skin_rgba(skin_64x64: &[u8]) -> Option<Image> {
    if skin_64x64.len() < 64 * 64 * 4 {
        return None;
    }
    let mut head_8x8 = [[0u8; 4]; 64];
    for y in 0..8usize {
        for x in 0..8usize {
            let base_idx = ((y + 8) * 64 + (x + 8)) * 4;
            let mut r = skin_64x64[base_idx];
            let mut g = skin_64x64[base_idx + 1];
            let mut b = skin_64x64[base_idx + 2];

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
    let face_rows: [[[u8; 3]; 8]; 8] = [
        [[46, 30, 18], [52, 34, 20], [58, 38, 22], [52, 34, 20], [46, 30, 18], [52, 34, 20], [58, 38, 22], [46, 30, 18]],
        [[46, 30, 18], [58, 38, 22], [64, 42, 25], [189, 142, 108], [189, 142, 108], [64, 42, 25], [58, 38, 22], [46, 30, 18]],
        [[46, 30, 18], [198, 150, 116], [208, 160, 124], [208, 160, 124], [208, 160, 124], [208, 160, 124], [198, 150, 116], [46, 30, 18]],
        [[198, 150, 116], [208, 160, 124], [208, 160, 124], [208, 160, 124], [208, 160, 124], [208, 160, 124], [208, 160, 124], [198, 150, 116]],
        [[198, 150, 116], [255, 255, 255], [68, 56, 168], [208, 160, 124], [208, 160, 124], [68, 56, 168], [255, 255, 255], [198, 150, 116]],
        [[189, 142, 108], [198, 150, 116], [208, 160, 124], [164, 114, 82], [164, 114, 82], [208, 160, 124], [198, 150, 116], [189, 142, 108]],
        [[180, 132, 98], [189, 142, 108], [118, 68, 42], [102, 56, 34], [102, 56, 34], [118, 68, 42], [189, 142, 108], [180, 132, 98]],
        [[172, 124, 90], [180, 132, 98], [102, 56, 34], [142, 88, 58], [142, 88, 58], [102, 56, 34], [180, 132, 98], [172, 124, 90]],
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
pub fn build_player_profiles() -> Vec<PlayerProfile> {
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
pub fn map_instance_to_entry(inst: &Instance, is_active: bool) -> InstanceEntry {
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
pub fn map_task_to_entry(task: &Task) -> TaskEntry {
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
pub fn build_changelog_entries() -> Vec<ChangelogEntry> {
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
pub fn map_game_version_to_entry(ver: &GameVersion) -> DownloadVersionEntry {
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

/// 构建 AI 页面「你可以这样问」8 项快捷提问列表（支持按批次切换“换一批”）
pub fn build_ai_prompt_entries(batch_index: usize) -> Vec<AiPromptEntry> {
    if batch_index.is_multiple_of(2) {
        vec![
            AiPromptEntry {
                id: "p-modpack".into(),
                title: "帮我做一个 1.21 的冒险整合包".into(),
                icon_kind: "modpack_box".into(),
                icon_color: Color::from_rgb_u8(245, 158, 11),
            },
            AiPromptEntry {
                id: "p-survive-mod".into(),
                title: "推荐适合生存的 Mod".into(),
                icon_kind: "puzzle".into(),
                icon_color: Color::from_rgb_u8(168, 85, 247),
            },
            AiPromptEntry {
                id: "p-shader".into(),
                title: "帮我配置一个光影包".into(),
                icon_kind: "mountain".into(),
                icon_color: Color::from_rgb_u8(56, 189, 248),
            },
            AiPromptEntry {
                id: "p-translate".into(),
                title: "汉化这个整合包".into(),
                icon_kind: "translate".into(),
                icon_color: Color::from_rgb_u8(59, 130, 246),
            },
            AiPromptEntry {
                id: "p-optimize".into(),
                title: "优化当前游戏配置".into(),
                icon_kind: "wrench".into(),
                icon_color: Color::from_rgb_u8(192, 132, 252),
            },
            AiPromptEntry {
                id: "p-datapack".into(),
                title: "写一个数据包指令".into(),
                icon_kind: "code_doc".into(),
                icon_color: Color::from_rgb_u8(129, 140, 248),
            },
            AiPromptEntry {
                id: "p-resource".into(),
                title: "推荐材质包".into(),
                icon_kind: "palette".into(),
                icon_color: Color::from_rgb_u8(244, 114, 182),
            },
            AiPromptEntry {
                id: "p-qa".into(),
                title: "游戏问题解答".into(),
                icon_kind: "help_bubble".into(),
                icon_color: Color::from_rgb_u8(45, 212, 191),
            },
        ]
    } else {
        vec![
            AiPromptEntry {
                id: "p-create".into(),
                title: "生成 1.20.1 机械动力整合包".into(),
                icon_kind: "modpack_box".into(),
                icon_color: Color::from_rgb_u8(245, 158, 11),
            },
            AiPromptEntry {
                id: "p-fps-mod".into(),
                title: "推荐高帧率优化 Mod 组合".into(),
                icon_kind: "puzzle".into(),
                icon_color: Color::from_rgb_u8(168, 85, 247),
            },
            AiPromptEntry {
                id: "p-low-shader".into(),
                title: "推荐核显可用的柔和光影".into(),
                icon_kind: "mountain".into(),
                icon_color: Color::from_rgb_u8(56, 189, 248),
            },
            AiPromptEntry {
                id: "p-mod-i18n".into(),
                title: "一键补全当前实例未汉化词条".into(),
                icon_kind: "translate".into(),
                icon_color: Color::from_rgb_u8(59, 130, 246),
            },
            AiPromptEntry {
                id: "p-jvm".into(),
                title: "优化 JVM 内存与 GC 启动参数".into(),
                icon_kind: "wrench".into(),
                icon_color: Color::from_rgb_u8(192, 132, 252),
            },
            AiPromptEntry {
                id: "p-loot".into(),
                title: "生成自定义战利品表数据包".into(),
                icon_kind: "code_doc".into(),
                icon_color: Color::from_rgb_u8(129, 140, 248),
            },
            AiPromptEntry {
                id: "p-pbr".into(),
                title: "推荐 PBR 写实风格材质包".into(),
                icon_kind: "palette".into(),
                icon_color: Color::from_rgb_u8(244, 114, 182),
            },
            AiPromptEntry {
                id: "p-crash".into(),
                title: "诊断最近一次崩溃日志原因".into(),
                icon_kind: "help_bubble".into(),
                icon_color: Color::from_rgb_u8(45, 212, 191),
            },
        ]
    }
}

/// 构建 AI 页面「实用示例」4 张展示卡片数据
pub fn build_ai_showcase_entries() -> Vec<AiShowcaseEntry> {
    vec![
        AiShowcaseEntry {
            id: "showcase-rpg".into(),
            title: "RPG 冒险整合包".into(),
            tag_1: "1.21".into(),
            tag_2: "魔法".into(),
            tag_3: "探索".into(),
            description: "自动生成配置并下载资源，附带 Mod 推荐与汉化。".into(),
            theme_kind: "rpg".into(),
        },
        AiShowcaseEntry {
            id: "showcase-tech".into(),
            title: "科技生存整合包".into(),
            tag_1: "1.21".into(),
            tag_2: "科技".into(),
            tag_3: "自动化".into(),
            description: "根据你的需求生成 Mod 列表，并完成下载、配置与汉化。".into(),
            theme_kind: "tech".into(),
        },
        AiShowcaseEntry {
            id: "showcase-shader".into(),
            title: "写实光影配置".into(),
            tag_1: "1.21".into(),
            tag_2: "光影".into(),
            tag_3: "画质优化".into(),
            description: "推荐并配置最适合的光影包，自动调整游戏设置。".into(),
            theme_kind: "shader".into(),
        },
        AiShowcaseEntry {
            id: "showcase-custom".into(),
            title: "自定义玩法".into(),
            tag_1: "1.21".into(),
            tag_2: "指令".into(),
            tag_3: "数据包".into(),
            description: "根据你的想法生成可直接使用的指令与数据包文件。".into(),
            theme_kind: "custom".into(),
        },
    ]
}

/// 构建 AI 页面右侧「任务列表」初始 3 个任务项数据
pub fn build_ai_task_entries() -> Vec<AiTaskEntry> {
    vec![
        AiTaskEntry {
            id: "ai-task-atm9".into(),
            title: "汉化整合包: All the Mods 9".into(),
            subtitle: "正在汉化中文语言文件...".into(),
            icon_kind: "modpack_box".into(),
            icon_color: Color::from_rgb_u8(245, 158, 11),
            bar_color: Color::from_rgb_u8(168, 85, 247),
            progress: 0.72,
            progress_label: "72%".into(),
            speed_text: "".into(),
            status: "running".into(),
            has_steps: true,
            step_1: "分析 Mod 列表".into(),
            step_1_state: "done".into(),
            step_2: "下载语言文件".into(),
            step_2_state: "done".into(),
            step_3: "翻译中 (142/198)".into(),
            step_3_state: "active".into(),
            step_4: "打包输出".into(),
            step_4_state: "pending".into(),
            step_5: "".into(),
            step_5_state: "".into(),
        },
        AiTaskEntry {
            id: "ai-task-sodium".into(),
            title: "下载 Mod: Sodium 0.5.11".into(),
            subtitle: "正在下载文件...".into(),
            icon_kind: "puzzle".into(),
            icon_color: Color::from_rgb_u8(0, 229, 139),
            bar_color: Color::from_rgb_u8(0, 229, 139),
            progress: 0.56,
            progress_label: "56%".into(),
            speed_text: "8.4 MB/s".into(),
            status: "running".into(),
            has_steps: false,
            step_1: "".into(),
            step_1_state: "".into(),
            step_2: "".into(),
            step_2_state: "".into(),
            step_3: "".into(),
            step_3_state: "".into(),
            step_4: "".into(),
            step_4_state: "".into(),
            step_5: "".into(),
            step_5_state: "".into(),
        },
        AiTaskEntry {
            id: "ai-task-shader".into(),
            title: "生成光影配置".into(),
            subtitle: "正在分析你的显卡与游戏设置...".into(),
            icon_kind: "mountain".into(),
            icon_color: Color::from_rgb_u8(56, 189, 248),
            bar_color: Color::from_rgb_u8(99, 102, 241),
            progress: 0.60,
            progress_label: "3/5".into(),
            speed_text: "".into(),
            status: "running".into(),
            has_steps: true,
            step_1: "检测硬件配置".into(),
            step_1_state: "done".into(),
            step_2: "推荐适合的光影包".into(),
            step_2_state: "done".into(),
            step_3: "优化参数".into(),
            step_3_state: "active".into(),
            step_4: "生成配置文件".into(),
            step_4_state: "pending".into(),
            step_5: "完成".into(),
            step_5_state: "pending".into(),
        },
    ]
}
