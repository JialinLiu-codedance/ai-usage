use std::sync::Mutex;

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, Rect, Size, WebviewWindow};

const PANEL_GAP: f64 = 0.0;

#[derive(Default)]
pub struct PanelAnchor {
    rect: Mutex<Option<Rect>>,
}

impl PanelAnchor {
    pub fn remember(&self, rect: Rect) {
        if let Ok(mut guard) = self.rect.lock() {
            *guard = Some(rect);
        }
    }

    fn current(&self) -> Option<Rect> {
        self.rect.lock().ok().and_then(|guard| *guard)
    }
}

pub fn resize_main_panel(app: &AppHandle, width: f64, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|error| format!("调整窗口尺寸失败: {error}"))?;
    position_main_panel(app, &window, width, height)?;

    Ok(())
}

pub fn position_main_panel(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let Some(anchor) = app.state::<PanelAnchor>().current() else {
        return Ok(());
    };

    let anchor_position = anchor_position(anchor);
    let anchor_size = anchor_size(anchor);
    let anchor_center_x = anchor_position.x + anchor_size.width / 2.0;
    let anchor_bottom_y = anchor_position.y + anchor_size.height;
    let monitor = app
        .available_monitors()
        .map_err(|error| format!("读取屏幕信息失败: {error}"))?
        .into_iter()
        .find(|monitor| {
            let work_area = monitor.work_area();
            let left = work_area.position.x as f64;
            let top = work_area.position.y as f64;
            let right = left + f64::from(work_area.size.width);
            let bottom = top + f64::from(work_area.size.height);
            anchor_center_x >= left
                && anchor_center_x <= right
                && anchor_bottom_y >= top
                && anchor_bottom_y <= bottom
        })
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return Ok(());
    };

    let scale_factor = monitor.scale_factor();
    let work_area = monitor.work_area();
    let panel_width = width * scale_factor;
    let panel_height = height * scale_factor;
    let gap = PANEL_GAP * scale_factor;

    let min_x = work_area.position.x as f64;
    let max_x = min_x + f64::from(work_area.size.width) - panel_width;
    let x = clamp_to_range(anchor_center_x - panel_width / 2.0, min_x, max_x);

    let min_y = work_area.position.y as f64;
    let max_y = min_y + f64::from(work_area.size.height) - panel_height;
    let below_y = anchor_bottom_y + gap;
    let above_y = anchor_position.y - panel_height - gap;
    let y = if below_y <= max_y { below_y } else { above_y };
    let y = clamp_to_range(y, min_y, max_y);

    window
        .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|error| format!("移动窗口失败: {error}"))?;

    Ok(())
}

fn clamp_to_range(value: f64, min: f64, max: f64) -> f64 {
    if max <= min {
        min
    } else {
        value.clamp(min, max)
    }
}

struct PhysicalPoint {
    x: f64,
    y: f64,
}

struct PhysicalExtent {
    width: f64,
    height: f64,
}

fn anchor_position(rect: Rect) -> PhysicalPoint {
    match rect.position {
        tauri::Position::Physical(position) => PhysicalPoint {
            x: position.x as f64,
            y: position.y as f64,
        },
        tauri::Position::Logical(position) => PhysicalPoint {
            x: position.x,
            y: position.y,
        },
    }
}

fn anchor_size(rect: Rect) -> PhysicalExtent {
    match rect.size {
        Size::Physical(size) => PhysicalExtent {
            width: f64::from(size.width),
            height: f64::from(size.height),
        },
        Size::Logical(size) => PhysicalExtent {
            width: size.width,
            height: size.height,
        },
    }
}
