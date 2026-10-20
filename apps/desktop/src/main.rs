use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::Arc;

use mc_core::{CreateInstanceRequest, InstanceService, MockInstanceService};

slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化核心服务层实例
    let service = Arc::new(MockInstanceService::new().with_sample_data().await);

    // 实例化主窗口
    let main_window = MainWindow::new()?;

    // 辅助函数：拉取实例列表并更新到 UI
    let update_ui_instances = {
        let window_weak = main_window.as_weak();
        let service = service.clone();
        move || {
            let window_weak = window_weak.clone();
            let service = service.clone();
            tokio::spawn(async move {
                if let Ok(instances) = service.list_instances().await {
                    let items: Vec<InstanceItem> = instances
                        .into_iter()
                        .map(|inst| InstanceItem {
                            id: inst.id.into(),
                            name: inst.name.into(),
                            mc_version: inst.mc_version.into(),
                            loader: inst
                                .loader
                                .map(|l| format!("{:?} {}", l.loader_type, l.version))
                                .unwrap_or_else(|| "纯净原版".to_string())
                                .into(),
                        })
                        .collect();

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = window_weak.upgrade() {
                            let model = Rc::new(VecModel::from(items));
                            window.set_instances(ModelRc::from(model));
                            window.set_status_text("实例同步完成".into());
                        }
                    });
                }
            });
        }
    };

    // 首次进入刷新实例
    update_ui_instances();

    // 绑定刷新按钮事件
    {
        let update_fn = update_ui_instances.clone();
        main_window.on_refresh_clicked(move || {
            update_fn();
        });
    }

    // 绑定创建新实例事件
    {
        let service = service.clone();
        let update_fn = update_ui_instances.clone();
        main_window.on_create_instance_clicked(move || {
            let service = service.clone();
            let update_fn = update_fn.clone();
            tokio::spawn(async move {
                let _ = service
                    .create_instance(CreateInstanceRequest {
                        name: "新建 1.21.1 纯净实例".to_string(),
                        mc_version: "1.21.1".to_string(),
                        loader: None,
                    })
                    .await;
                update_fn();
            });
        });
    }

    // 启动原生主事件循环
    main_window.run()?;

    Ok(())
}
