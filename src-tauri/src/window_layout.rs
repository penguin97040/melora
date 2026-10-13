use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow};

const PREFERRED: LogicalSize<f64> = LogicalSize::new(1440.0, 940.0);
const MINIMUM: LogicalSize<f64> = LogicalSize::new(720.0, 520.0);

struct OpeningLayout {
    size: PhysicalSize<u32>,
    minimum: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
}

fn opening_layout(
    work_area: tauri::PhysicalRect<i32, u32>,
    scale: f64,
    decorations: PhysicalSize<u32>,
) -> OpeningLayout {
    // Work area and decoration measurements are physical pixels. Preferred
    // dimensions are logical pixels, independently of the frontend interface scale.
    let available = PhysicalSize::new(
        work_area
            .size
            .width
            .saturating_sub(decorations.width)
            .max(1),
        work_area
            .size
            .height
            .saturating_sub(decorations.height)
            .max(1),
    );
    let size = PhysicalSize::new(
        ((PREFERRED.width * scale).round() as u32).min(available.width),
        ((PREFERRED.height * scale).round() as u32).min(available.height),
    );
    OpeningLayout {
        size,
        // A minimum larger than the work area would defeat the opening bound.
        minimum: PhysicalSize::new(
            ((MINIMUM.width * scale).round() as u32).min(size.width),
            ((MINIMUM.height * scale).round() as u32).min(size.height),
        ),
        position: PhysicalPosition::new(
            work_area.position.x
                + (work_area
                    .size
                    .width
                    .saturating_sub(size.width + decorations.width)
                    / 2) as i32,
            work_area.position.y
                + (work_area
                    .size
                    .height
                    .saturating_sub(size.height + decorations.height)
                    / 2) as i32,
        ),
    }
}

pub fn fit_opening_window(window: &WebviewWindow) -> tauri::Result<()> {
    let monitor = window.current_monitor()?.or(window.primary_monitor()?);
    if let Some(monitor) = monitor {
        let inner = window.inner_size()?;
        let outer = window.outer_size()?;
        let layout = opening_layout(
            *monitor.work_area(),
            monitor.scale_factor(),
            PhysicalSize::new(
                outer.width.saturating_sub(inner.width),
                outer.height.saturating_sub(inner.height),
            ),
        );
        window.set_min_size(Some(layout.minimum))?;
        window.set_size(layout.size)?;
        window.set_position(layout.position)?;
    }
    Ok(())
}

#[derive(Default)]
pub struct StartupWindow {
    shown: bool,
}

impl StartupWindow {
    pub fn reveal<E>(&mut self, finish_and_show: impl FnOnce() -> Result<(), E>) -> Result<(), E> {
        if !self.shown {
            finish_and_show()?;
            self.shown = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_configs_preserve_hidden_opening_and_native_chrome_contract() {
        for (source, macos) in [
            (include_str!("../tauri.conf.json"), false),
            (include_str!("../tauri.macos.conf.json"), true),
        ] {
            let config: serde_json::Value = serde_json::from_str(source).unwrap();
            let window: tauri::utils::config::WindowConfig =
                serde_json::from_value(config["app"]["windows"][0].clone()).unwrap();
            assert_eq!(window.label, "main");
            assert_eq!(
                (window.width, window.height),
                (PREFERRED.width, PREFERRED.height)
            );
            assert!(!window.visible);
            assert!(!window.create);
            assert!(window.resizable);
            assert_eq!(window.decorations, macos);
            if macos {
                assert!(window.hidden_title);
                assert!(matches!(
                    window.title_bar_style,
                    tauri::TitleBarStyle::Overlay
                ));
                assert!(window.traffic_light_position.is_some());
            }
        }
    }

    #[test]
    fn preferred_client_size_reserves_decorations_and_centres_in_work_area() {
        let layout = opening_layout(
            tauri::PhysicalRect {
                position: PhysicalPosition::new(-1920, 40),
                size: PhysicalSize::new(1920, 1040),
            },
            1.0,
            PhysicalSize::new(16, 39),
        );
        assert_eq!(layout.size, PhysicalSize::new(1440, 940));
        assert_eq!(layout.position, PhysicalPosition::new(-1688, 70));
    }

    #[test]
    fn scaled_and_small_displays_bound_both_size_and_minimum() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let area = tauri::PhysicalRect {
                position: PhysicalPosition::new(0, 30),
                size: PhysicalSize::new(1280, 690),
            };
            let decorations = PhysicalSize::new(16, (39.0 * scale) as u32);
            let layout = opening_layout(area, scale, decorations);
            assert!(layout.size.width + decorations.width <= area.size.width);
            assert!(layout.size.height + decorations.height <= area.size.height);
            assert!(layout.minimum.width <= layout.size.width);
            assert!(layout.minimum.height <= layout.size.height);
            assert!(layout.position.x >= area.position.x);
            assert!(layout.position.y >= area.position.y);
        }
    }

    #[test]
    fn high_dpi_large_display_preserves_logical_preferred_size() {
        let layout = opening_layout(
            tauri::PhysicalRect {
                position: PhysicalPosition::new(0, 0),
                size: PhysicalSize::new(3840, 2160),
            },
            2.0,
            PhysicalSize::new(32, 78),
        );
        assert_eq!(layout.size, PhysicalSize::new(2880, 1880));
        assert_eq!(layout.minimum, PhysicalSize::new(1440, 1040));
    }

    #[test]
    fn duplicate_readiness_does_not_resize_or_reveal_again() {
        let mut startup = StartupWindow::default();
        let mut operations = Vec::new();
        startup
            .reveal(|| {
                operations.extend(["fit", "show"]);
                Ok::<_, ()>(())
            })
            .unwrap();
        operations.push("user resize");
        startup
            .reveal(|| {
                operations.extend(["fit", "show"]);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(operations, ["fit", "show", "user resize"]);
    }

    #[test]
    fn failed_reveal_can_be_retried() {
        let mut startup = StartupWindow::default();
        assert_eq!(
            startup.reveal(|| Err("fixture failure")),
            Err("fixture failure")
        );
        assert!(!startup.shown);
        assert_eq!(startup.reveal(|| Ok::<_, ()>(())), Ok(()));
        assert!(startup.shown);
    }
}
