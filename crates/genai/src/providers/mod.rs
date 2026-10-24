//! Inpainting services behind [`crate::ImageProvider`], one module each.

pub mod bfl;
pub mod bria;
pub mod comfyui;
pub mod common;
pub mod compatible;
pub(crate) mod configured;
pub mod fal;
pub mod firefly;
pub mod flux_fill;
pub mod ideogram;
pub mod openai;
pub mod replicate;
pub mod stability;
