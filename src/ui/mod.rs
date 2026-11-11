pub mod app_icon;
pub mod card_surface;
pub mod cat_style;
pub mod cli_input;
pub mod device_menu;
pub mod dirty;
#[cfg(debug_assertions)]
pub mod demo;
pub mod dock;
pub mod editor;
pub mod editor_layout;
pub mod editor_paint;
pub mod editor_popover;
#[cfg(test)]
mod font_coverage;
pub mod fonts;
pub mod frame_clock;
pub mod geometry;
pub mod hotkey_rec;
#[cfg(test)]
mod icon_frames;
pub mod icons;
pub mod menu_pop;
pub mod month_grid;
pub mod motion;
pub mod notch_fill;
pub mod panel_layout;
pub mod phone_view;
pub mod render;
pub mod robot;
pub mod settings;
pub mod settings_controls;
pub mod settings_layout;
pub mod settings_window;
pub mod shelf_motion;
pub mod tray;
pub mod transcript_menu;
pub mod tray_dot;
pub mod window;
