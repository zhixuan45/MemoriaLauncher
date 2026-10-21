//! Opt-in rendered screenshots and interaction checks using the real iced event loop.
//! cargo run -p desktop -- --screenshot docs/screenshots/home.png
//! cargo run -p desktop -- --ui-smoke docs/screenshots
use crate::app::{Card, Launcher, Message, Page};
use iced::{window, Size, Task};
use std::{path::PathBuf, time::Duration};

#[derive(Default)]
pub struct Verification {
    output: Option<PathBuf>,
    smoke: bool,
    step: usize,
}

fn argument(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone())
}

impl Verification {
    pub fn from_args() -> Self {
        let smoke = argument("--ui-smoke");
        Self {
            output: smoke
                .clone()
                .or_else(|| argument("--screenshot"))
                .map(Into::into),
            smoke: smoke.is_some(),
            step: 0,
        }
    }
    pub fn enabled(&self) -> bool {
        self.output.is_some()
    }
}

pub fn window_size() -> Size {
    argument("--size")
        .and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some(Size::new(width.parse().ok()?, height.parse().ok()?))
        })
        .unwrap_or(Size::new(1280., 768.))
}

fn delayed(message: Message) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(900)).await;
            message
        },
        |message| message,
    )
}

pub fn after_load(app: &mut Launcher) -> Task<Message> {
    if app.verification.smoke {
        assert_eq!(app.instances.len(), 4);
        assert_eq!(app.tasks.len(), 2);
        assert_eq!(app.current().unwrap().instance.name, "Fabric 1.21");
    }
    delayed(Message::Capture)
}

pub fn save(app: &mut Launcher, screenshot: window::Screenshot) -> Task<Message> {
    let Some(output) = &app.verification.output else {
        return Task::none();
    };
    let path = if app.verification.smoke {
        output.join(format!(
            "{:02}-{}.png",
            app.verification.step,
            match app.verification.step {
                0 => "home",
                1 => "selected-paused",
                2 => "layout",
                3 => "download-pending",
                4 => "ai-pending",
                5 => "settings-aligned",
                6 => "instance-dialog",
                _ => "restored",
            }
        ))
    } else {
        output.clone()
    };
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image::save_buffer(
            &path,
            &screenshot.rgba,
            screenshot.size.width,
            screenshot.size.height,
            image::ColorType::Rgba8,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Screenshot failed: {error}");
        std::process::exit(1);
    }
    println!("Screenshot: {}", path.display());
    if app.verification.smoke {
        delayed(Message::VerificationNext)
    } else {
        iced::exit()
    }
}

pub fn next(app: &mut Launcher) -> Task<Message> {
    app.verification.step += 1;
    // Dispatch the same messages as real widgets, then check the state before capture.
    match app.verification.step {
        1 => {
            let _ = app.update(Message::SelectInstance("inst-vanilla-121".into()));
            let _ = app.update(Message::ToggleAllTasks);
            let _ = app.update(Message::Feature(1));
            assert_eq!(app.current().unwrap().instance.name, "原版 1.21");
            assert!(app.tasks.iter().all(|t| t.paused));
            assert_eq!(app.feature, 1);
        }
        2 => {
            let _ = app.update(Message::ToggleLayout);
            let _ = app.update(Message::HideCard(Card::Stats));
            app.cursor = iced::Point::new(450., 250.);
            let _ = app.update(Message::BeginDrag(Card::Instances, false));
            let _ = app.update(Message::Event(iced::Event::Mouse(
                iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(475., 250.),
                },
            )));
            assert_eq!(app.layouts[Card::Instances.index()].offset.x, 32.);
            let _ = app.update(Message::Event(iced::Event::Mouse(
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            )));
            assert!(!app.layouts[Card::Stats.index()].visible);
            assert!(app.drag.is_none());
            let _ = app.update(Message::BeginDrag(Card::Queue, true));
            let _ = app.update(Message::Event(iced::Event::Mouse(
                iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(475., 225.),
                },
            )));
            assert_eq!(app.layouts[Card::Queue.index()].extra.y, -32.);
            let _ = app.update(Message::Event(iced::Event::Mouse(
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            )));
            let _ = app.update(Message::MoveInstance("inst-vanilla-121".into(), -1));
            assert_eq!(app.instances[0].instance.id, "inst-vanilla-121");
        }
        3 => {
            let _ = app.update(Message::Navigate(Page::Download));
        }
        4 => {
            let _ = app.update(Message::Navigate(Page::Ai));
        }
        5 => {
            let _ = app.update(Message::Navigate(Page::Settings));
        }
        6 => {
            let _ = app.update(Message::Navigate(Page::Home));
            let _ = app.update(Message::ResetLayout);
            let _ = app.update(Message::ToggleLayout);
            let _ = app.update(Message::EditInstance);
        }
        7 => {
            let _ = app.update(Message::Dismiss);
            let _ = app.update(Message::ResetLayout);
            let _ = app.update(Message::ToggleAllTasks);
            let _ = app.update(Message::SelectInstance("inst-fabric-121".into()));
            let _ = app.update(Message::Feature(-1));
            let _ = app.update(Message::MoveInstance("inst-fabric-121".into(), -1));
            assert!(app
                .layouts
                .iter()
                .all(|l| l.visible && l.offset == iced::Vector::ZERO));
            assert!(app.tasks.iter().all(|t| !t.paused));
        }
        _ => {
            println!("UI smoke checks passed.");
            return iced::exit();
        }
    }
    delayed(Message::Capture)
}
