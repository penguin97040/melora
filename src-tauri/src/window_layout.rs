use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow};

const PREFERRED: LogicalSize<f64> = LogicalSize::new(1360.0, 940.0);
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

pub fn initialise(window: &WebviewWindow) -> tauri::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};

    fit_opening_window(window)?;
    let realised = AtomicBool::new(false);
    let opening_window = window.clone();
    // Some window managers report decorations only after the first native
    // configure/resize event. Re-measure once, then leave user resizing alone.
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Resized(_))
            && !realised.swap(true, Ordering::Relaxed)
            && fit_opening_window(&opening_window).is_err()
        {
            eprintln!("Melora could not fit the opening window to the monitor work area.");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(layout.size, PhysicalSize::new(1360, 940));
        assert_eq!(layout.position, PhysicalPosition::new(-1648, 70));
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
        assert_eq!(layout.size, PhysicalSize::new(2720, 1880));
        assert_eq!(layout.minimum, PhysicalSize::new(1440, 1040));
    }
}
