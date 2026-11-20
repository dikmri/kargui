//! kargui: a light native GUI framework for Rust (work in progress).
#![allow(dead_code)]

mod damage;
mod gdi;
mod geom;
mod gpu;
pub mod log;
mod paint;
mod raster;
mod text;
pub mod ui;

pub use geom::{Color, IRect, Rect, rgb};
pub use log::set_logger;
