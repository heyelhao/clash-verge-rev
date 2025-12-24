#[cfg(target_os = "windows")]
use crate::utils::dirs::PathBufExec as _;
#[cfg(target_os = "windows")]
use crate::utils::schtasks as startup_task;
use crate::{config::Config, core::handle::Handle};
use anyhow::{Result, anyhow};
#[cfg(not(target_os = "windows"))]
use clash_verge_logging::logging_error;
use clash_verge_logging::{Type, logging};
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use tauri_plugin_autostart::ManagerExt as _;
#[cfg(target_os = "windows")]
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

#[cfg(target_os = "windows")]
fn get_startup_dir() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA").map_err(|_| anyhow!("failed to read APPDATA env var"))?;
    let startup_dir = PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup");

    if !startup_dir.exists() {
        return Err(anyhow!("startup folder does not exist: {:?}", startup_dir));
    }

    Ok(startup_dir)
}

#[cfg(target_os = "windows")]
async fn cleanup_legacy_startup_shortcuts() -> Result<()> {
    let startup_dir = get_startup_dir()?;
    let old_shortcut = startup_dir.join("Clash-Verge.lnk");
    let new_shortcut = startup_dir.join("Clash Verge.lnk");

    old_shortcut.remove_if_exists().await?;
    new_shortcut.remove_if_exists().await?;
    Ok(())
}

/// Update autostart state from current configuration.
pub async fn update_launch() -> Result<()> {
    let enable_auto_launch = { Config::verge().await.latest_arc().enable_auto_launch };
    let is_enable = enable_auto_launch.unwrap_or(false);
    logging!(info, Type::System, "Setting auto-launch state to: {:?}", is_enable);

    #[cfg(target_os = "windows")]
    {
        if let Err(err) = cleanup_legacy_startup_shortcuts().await {
            logging!(warn, Type::Setup, "Failed to cleanup legacy startup shortcuts: {}", err);
        }

        let is_admin = is_current_app_handle_admin(Handle::app_handle());
        if is_admin {
            update_launch_for_admin(is_enable)
        } else {
            update_launch_for_user(is_enable)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        update_autostart_non_windows(is_enable);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn autostart_plugin_enabled() -> Result<bool> {
    let app_handle = Handle::app_handle();
    let autostart_manager = app_handle.autolaunch();
    autostart_manager
        .is_enabled()
        .map_err(|e| anyhow!("Failed to get autostart plugin status: {}", e))
}

#[cfg(target_os = "windows")]
fn set_autostart_plugin_state(is_enable: bool) -> Result<()> {
    let app_handle = Handle::app_handle();
    let autostart_manager = app_handle.autolaunch();

    if is_enable {
        autostart_manager
            .enable()
            .map_err(|e| anyhow!("Failed to enable auto launch: {}", e))?;
    } else {
        autostart_manager
            .disable()
            .map_err(|e| anyhow!("Failed to disable auto launch: {}", e))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn update_launch_for_admin(is_enable: bool) -> Result<()> {
    let plugin_enabled = autostart_plugin_enabled()?;
    if plugin_enabled {
        set_autostart_plugin_state(false)?;
    }

    if let Err(err) = startup_task::set_auto_launch(is_enable) {
        if plugin_enabled && let Err(rollback_err) = set_autostart_plugin_state(true) {
            logging!(
                warn,
                Type::Setup,
                "Failed to rollback autostart plugin state: {}",
                rollback_err
            );
        }
        return Err(err);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn update_launch_for_user(is_enable: bool) -> Result<()> {
    if startup_task::is_task_enabled()? {
        return Err(anyhow!(
            "admin auto-launch task exists; run the app as administrator to remove it"
        ));
    }

    let plugin_enabled = autostart_plugin_enabled()?;
    if plugin_enabled != is_enable {
        set_autostart_plugin_state(is_enable)?;
    }

    Ok(())
}

/// Update autostart using platform defaults on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
fn update_autostart_non_windows(is_enable: bool) {
    let app_handle = Handle::app_handle();
    let autostart_manager = app_handle.autolaunch();

    if is_enable {
        logging_error!(Type::System, "{:?}", autostart_manager.enable());
    } else {
        logging_error!(Type::System, "{:?}", autostart_manager.disable());
    }
}

/// Get current autostart status.
pub fn get_launch_status() -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        let admin_enabled = startup_task::is_task_enabled()?;
        if admin_enabled {
            logging!(
                info,
                Type::System,
                "Auto launch status (scheduled task admin): {admin_enabled}"
            );
            return Ok(true);
        }

        match autostart_plugin_enabled() {
            Ok(status) => {
                logging!(info, Type::System, "Auto launch status (autostart plugin): {status}");
                Ok(status)
            }
            Err(err) => {
                logging!(error, Type::System, "Failed to get autostart plugin status: {}", err);
                Err(err)
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let app_handle = Handle::app_handle();
        let autostart_manager = app_handle.autolaunch();
        match autostart_manager.is_enabled() {
            Ok(status) => {
                logging!(info, Type::System, "Auto launch status: {status}");
                Ok(status)
            }
            Err(e) => {
                logging!(error, Type::System, "Failed to get auto launch status: {e}");
                Err(anyhow!("Failed to get auto launch status: {}", e))
            }
        }
    }
}
