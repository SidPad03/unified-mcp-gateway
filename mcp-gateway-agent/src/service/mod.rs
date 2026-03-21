#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

fn agent_bin_path() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| crate::config::bin_dir().join("mcp-gateway-agent"))
        .to_string_lossy()
        .to_string()
}
